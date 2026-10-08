//! Per-mode setting-table key dispatchers.
//!
//! Every shell stores its UI state in the same `settings` table
//! (key/value rows). For knobs that exist *per mode* (signal-mode,
//! keep-screen-awake, stopwatch toggle, label active, default label
//! UUID), the key string is a function of the mode. Keeping these in
//! one place means the GTK shell, the Android shell, and any future
//! shell read and write to the same rows.
//!
//! All functions take `SessionMode` (the canonical mode enum) and
//! return a stable `&'static str` key. The keys are wire format:
//! never edit one without a DB migration.

use crate::db::{Database, SessionMode, SignalMode};

/// Signal-mode override: which channels (sound / vibration / both /
/// neither) are allowed to fire for each mode. Per-bell signal_mode
/// AND-combines with this to decide whether a particular bell rings.
pub fn signal_mode_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "timer_signal_mode",
        SessionMode::Guided => "guided_signal_mode",
        SessionMode::BoxBreath => "boxbreath_signal_mode",
    }
}

/// Per-mode keep-screen-awake toggle. Each mode persists independently
/// because their session pacing differs (Timer counts down, Box Breath
/// runs at the user's chosen pace, Guided plays a file).
pub fn keep_screen_awake_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "timer_keep_screen_awake",
        SessionMode::Guided => "guided_keep_screen_awake",
        SessionMode::BoxBreath => "boxbreath_keep_screen_awake",
    }
}

/// Per-mode End Bell config (2026-07-17, was one flat key set
/// shared across modes in both shells — a design bug: a guided
/// track and a Timer session should be able to want different end
/// chimes). Four keys per mode; no fallback to the dead flat
/// `end_bell_*` keys (pre-publication no-compat policy — reconfigure
/// once per mode).
pub fn end_bell_active_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "timer_end_bell_active",
        SessionMode::Guided => "guided_end_bell_active",
        SessionMode::BoxBreath => "boxbreath_end_bell_active",
    }
}

/// Per-mode End Bell sound uuid (see `end_bell_active_key_for_mode`).
pub fn end_bell_sound_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "timer_end_bell_sound",
        SessionMode::Guided => "guided_end_bell_sound",
        SessionMode::BoxBreath => "boxbreath_end_bell_sound",
    }
}

/// Per-mode End Bell vibration-pattern uuid.
pub fn end_bell_pattern_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "timer_end_bell_pattern",
        SessionMode::Guided => "guided_end_bell_pattern",
        SessionMode::BoxBreath => "boxbreath_end_bell_pattern",
    }
}

/// Per-mode End Bell volume, in percent (`crate::bell_volume`).
pub fn end_bell_volume_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "timer_end_bell_volume",
        SessionMode::Guided => "guided_end_bell_volume",
        SessionMode::BoxBreath => "boxbreath_end_bell_volume",
    }
}

/// Starting Bell volume, in percent (`crate::bell_volume`). The
/// starting bell is Timer-only, so one key.
pub const STARTING_BELL_VOLUME_KEY: &str = "starting_bell_volume";

/// Per-mode End Bell type (sound / vibration / both).
pub fn end_bell_signal_mode_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "timer_end_bell_signal_mode",
        SessionMode::Guided => "guided_end_bell_signal_mode",
        SessionMode::BoxBreath => "boxbreath_end_bell_signal_mode",
    }
}

/// Per-mode stopwatch-active toggle. Each mode has its own stopwatch
/// concept (Timer counts up; Box Breath runs without a target;
/// Guided plays without an auto-end-bell at file EOS), so they don't
/// share a flag.
pub fn stopwatch_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "timer_stopwatch_active",
        SessionMode::Guided => "guided_stopwatch_active",
        SessionMode::BoxBreath => "boxbreath_stopwatch_active",
    }
}

/// Per-mode "label expander on/off" toggle.
pub fn label_active_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "label_active_timer",
        SessionMode::BoxBreath => "label_active_breathing",
        SessionMode::Guided => "label_active_guided",
    }
}

/// Per-mode persisted-label-choice key. Stores the UUID of the label
/// the user last picked in this mode.
pub fn label_uuid_key_for_mode(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Timer => "default_label_uuid_timer",
        SessionMode::BoxBreath => "default_label_uuid_breathing",
        SessionMode::Guided => "default_label_uuid_guided",
    }
}

/// Default-label UUID for each mode — the seeded row a freshly opened
/// app falls back to when no per-mode choice has been persisted yet.
/// Resolves through the bundled UUIDs in `crate::seeds`.
pub fn default_label_uuid_for_mode(mode: SessionMode) -> &'static str {
    use crate::seeds::{
        DEFAULT_BREATHING_LABEL_UUID, DEFAULT_GUIDED_LABEL_UUID, DEFAULT_TIMER_LABEL_UUID,
    };
    match mode {
        SessionMode::Timer => DEFAULT_TIMER_LABEL_UUID,
        SessionMode::BoxBreath => DEFAULT_BREATHING_LABEL_UUID,
        SessionMode::Guided => DEFAULT_GUIDED_LABEL_UUID,
    }
}

/// Settings-row boolean parse. Every boolean-valued settings row is
/// stored as the literal string "true" or "false" (see the wider
/// project convention; the `settings` table has no type info). Anything
/// other than "true" reads as false — matches the existing per-call
/// `db.get_setting(k, "false") == "true"` idiom.
pub fn parse_bool(s: &str) -> bool {
    s == "true"
}

/// Settings-row boolean render. Inverse of `parse_bool`; the literal
/// strings the `settings` table stores.
pub fn format_bool(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

/// Read a boolean settings row, falling back to `default` when the
/// row is missing OR the stored value doesn't parse as `"true"`.
/// Collapses the inline `db.get_setting(k, …) → ok → parse_bool →
/// unwrap_or(default)` chain that recurs in every shell on every
/// per-mode toggle, master-feature toggle, and screen-awake flag.
/// The `default`-as-fallback-string handling stays inside the helper
/// so callers only express their domain default.
pub fn read_bool(db: &Database, key: &str, default: bool) -> bool {
    db.get_setting(key, format_bool(default))
        .map_or(default, |v| parse_bool(&v))
}

/// Read a string settings row, falling back to `default` when the
/// row is missing. Sibling of `read_bool` for `String`-shaped values
/// (sound UUIDs, vibration-pattern UUIDs, etc.).
pub fn read_str(db: &Database, key: &str, default: &str) -> String {
    db.get_setting(key, default).unwrap_or_else(|_| default.to_string())
}

/// Read a `SignalMode` settings row, falling back to `default` when
/// the row is missing or carries a string that doesn't parse as one
/// of the known variants. The `default.as_db_str()` is passed as the
/// `get_setting` fallback so a fresh DB doesn't perturb the
/// canonical default value at the get_setting level either.
pub fn read_signal_mode(db: &Database, key: &str, default: SignalMode) -> SignalMode {
    db.get_setting(key, default.as_db_str())
        .ok()
        .and_then(|s| SignalMode::from_db_str(&s))
        .unwrap_or(default)
}

/// Read a `u32` settings row, falling back to `default` when the
/// row is missing or the stored value isn't a parseable u32.
pub fn read_u32(db: &Database, key: &str, default: u32) -> u32 {
    read_str(db, key, &default.to_string())
        .parse::<u32>()
        .unwrap_or(default)
}

/// Per-mode keep-screen-awake reader. Shell calls this on visit
/// (sync the switch UI) and at session start (decide whether to
/// hold the idle-inhibit cookie). Two callsites in the gtk shell
/// today; Android will have its own pair.
pub fn keep_screen_awake_from_db(db: &Database, mode: SessionMode) -> bool {
    read_bool(db, keep_screen_awake_key_for_mode(mode), false)
}

/// Timer mode's countdown length, kept across launches and synced.
pub fn timer_session_secs_from_db(db: &Database) -> u32 {
    read_u32(db, "timer_session_secs", crate::session::TIMER_DEFAULT_SECS)
}

pub fn set_timer_session_secs(db: &Database, secs: u32) -> crate::db::Result<()> {
    db.set_setting("timer_session_secs", &secs.to_string())
}

/// Box Breath's phase pattern and session length, clamped so a value
/// out of range (from sync or an old version) still loads cleanly.
pub fn breathing_from_db(db: &Database) -> (crate::breath::BreathPattern, u32) {
    let pattern = crate::breath::BreathPattern::clamp_from_raw(
        read_u32(db, "breathing_in", 4),
        read_u32(db, "breathing_hold_in", 4),
        read_u32(db, "breathing_out", 4),
        read_u32(db, "breathing_hold_out", 4),
    );
    let secs = read_u32(db, "breathing_session_secs", crate::session::BREATHING_DEFAULT_SECS);
    (pattern, crate::breath::clamp_session_secs(secs))
}

pub fn set_breathing(
    db: &Database,
    pattern: crate::breath::BreathPattern,
    secs: u32,
) -> crate::db::Result<()> {
    db.set_setting("breathing_in", &pattern.in_secs.to_string())?;
    db.set_setting("breathing_hold_in", &pattern.hold_in.to_string())?;
    db.set_setting("breathing_out", &pattern.out_secs.to_string())?;
    db.set_setting("breathing_hold_out", &pattern.hold_out.to_string())?;
    db.set_setting("breathing_session_secs", &secs.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of these dispatchers is that no two modes
    /// share a key — otherwise the per-mode toggles would leak into
    /// each other (e.g. a stopwatch flip in Timer would also flip the
    /// Guided stopwatch).
    fn assert_three_distinct(keys: [&str; 3]) {
        assert_ne!(keys[0], keys[1], "Timer and the second mode share key");
        assert_ne!(keys[0], keys[2], "Timer and the third mode share key");
        assert_ne!(keys[1], keys[2], "second and third modes share key");
    }

    #[test]
    fn signal_mode_keys_are_distinct_per_mode() {
        assert_three_distinct([
            signal_mode_key_for_mode(SessionMode::Timer),
            signal_mode_key_for_mode(SessionMode::Guided),
            signal_mode_key_for_mode(SessionMode::BoxBreath),
        ]);
    }

    #[test]
    fn end_bell_keys_are_distinct_per_mode_and_per_field() {
        // 15 keys total (5 fields × 3 modes) — all must be unique,
        // and none may collide with the dead flat `end_bell_*`
        // keys (which would silently resurrect the shared-config
        // bug for one mode).
        let keys: Vec<&str> = [SessionMode::Timer, SessionMode::Guided, SessionMode::BoxBreath]
            .into_iter()
            .flat_map(|m| {
                [
                    end_bell_active_key_for_mode(m),
                    end_bell_sound_key_for_mode(m),
                    end_bell_pattern_key_for_mode(m),
                    end_bell_signal_mode_key_for_mode(m),
                    end_bell_volume_key_for_mode(m),
                ]
            })
            .collect();
        let unique: std::collections::HashSet<&&str> = keys.iter().collect();
        assert_eq!(unique.len(), 15, "every mode+field key must be unique");
        assert!(!keys.contains(&STARTING_BELL_VOLUME_KEY));
        for dead in ["end_bell_active", "end_bell_sound", "end_bell_pattern", "end_bell_signal_mode"] {
            assert!(!keys.contains(&dead), "must not reuse dead flat key {dead}");
        }
    }

    #[test]
    fn keep_screen_awake_keys_are_distinct_per_mode() {
        assert_three_distinct([
            keep_screen_awake_key_for_mode(SessionMode::Timer),
            keep_screen_awake_key_for_mode(SessionMode::Guided),
            keep_screen_awake_key_for_mode(SessionMode::BoxBreath),
        ]);
    }

    #[test]
    fn stopwatch_keys_are_distinct_per_mode() {
        assert_three_distinct([
            stopwatch_key_for_mode(SessionMode::Timer),
            stopwatch_key_for_mode(SessionMode::Guided),
            stopwatch_key_for_mode(SessionMode::BoxBreath),
        ]);
    }

    #[test]
    fn label_active_keys_are_distinct_per_mode() {
        assert_three_distinct([
            label_active_key_for_mode(SessionMode::Timer),
            label_active_key_for_mode(SessionMode::Guided),
            label_active_key_for_mode(SessionMode::BoxBreath),
        ]);
    }

    #[test]
    fn parse_bool_only_true_returns_true() {
        assert!(parse_bool("true"));
        assert!(!parse_bool("false"));
        assert!(!parse_bool(""));
        assert!(!parse_bool("True"));
        assert!(!parse_bool("1"));
    }

    #[test]
    fn format_bool_round_trips_through_parse_bool() {
        assert!(parse_bool(format_bool(true)));
        assert!(!parse_bool(format_bool(false)));
    }

    #[test]
    fn read_bool_returns_default_when_key_missing() {
        let db = Database::open_in_memory().unwrap();
        assert!(read_bool(&db, "absent", true));
        assert!(!read_bool(&db, "absent", false));
    }

    #[test]
    fn read_bool_reads_persisted_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("flag", "true").unwrap();
        assert!(read_bool(&db, "flag", false));
        db.set_setting("flag", "false").unwrap();
        assert!(!read_bool(&db, "flag", true));
    }

    #[test]
    fn read_str_returns_default_when_key_missing() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(read_str(&db, "absent", "fallback"), "fallback");
    }

    #[test]
    fn read_str_reads_persisted_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("k", "stored").unwrap();
        assert_eq!(read_str(&db, "k", "default"), "stored");
    }

    #[test]
    fn read_signal_mode_returns_default_when_key_missing() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(read_signal_mode(&db, "absent", SignalMode::Both), SignalMode::Both);
    }

    #[test]
    fn read_signal_mode_returns_default_on_unrecognised_value() {
        // Defensive: a corrupted row or a string from a future build
        // that adds variants should fall back, not panic.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("k", "future_variant").unwrap();
        assert_eq!(read_signal_mode(&db, "k", SignalMode::Sound), SignalMode::Sound);
    }

    #[test]
    fn read_signal_mode_reads_every_known_variant() {
        let db = Database::open_in_memory().unwrap();
        for variant in [SignalMode::Sound, SignalMode::Vibration, SignalMode::Both] {
            db.set_setting("k", variant.as_db_str()).unwrap();
            assert_eq!(read_signal_mode(&db, "k", SignalMode::Both), variant);
        }
    }

    #[test]
    fn read_u32_returns_default_when_key_missing() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(read_u32(&db, "absent", 600), 600);
    }

    #[test]
    fn read_u32_returns_default_on_unparseable_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("k", "not_a_number").unwrap();
        assert_eq!(read_u32(&db, "k", 42), 42);
        // Negative input doesn't parse as u32 either.
        db.set_setting("k", "-7").unwrap();
        assert_eq!(read_u32(&db, "k", 42), 42);
    }

    #[test]
    fn read_u32_reads_persisted_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("k", "1234").unwrap();
        assert_eq!(read_u32(&db, "k", 0), 1234);
    }

    #[test]
    fn session_timing_defaults_when_nothing_is_stored() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(timer_session_secs_from_db(&db), crate::session::TIMER_DEFAULT_SECS);
        assert_eq!(
            breathing_from_db(&db),
            (crate::breath::BreathPattern::box_breath(), crate::session::BREATHING_DEFAULT_SECS),
        );
    }

    #[test]
    fn session_timing_round_trips_under_the_synced_keys() {
        let db = Database::open_in_memory().unwrap();
        set_timer_session_secs(&db, 1500).unwrap();
        let p = crate::breath::BreathPattern::clamp_from_raw(5, 2, 7, 0);
        set_breathing(&db, p, 900).unwrap();
        assert_eq!(timer_session_secs_from_db(&db), 1500);
        assert_eq!(breathing_from_db(&db), (p, 900));
        // Peers on 26.10.1 read these exact keys.
        for (k, v) in [("timer_session_secs", "1500"), ("breathing_in", "5"), ("breathing_hold_in", "2"),
            ("breathing_out", "7"), ("breathing_hold_out", "0"), ("breathing_session_secs", "900")] {
            assert_eq!(db.get_setting(k, "").unwrap(), v, "{k}");
        }
    }

    #[test]
    fn stored_breathing_values_out_of_range_are_clamped() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("breathing_session_secs", "5").unwrap();
        db.set_setting("breathing_in", "99").unwrap();
        let (p, secs) = breathing_from_db(&db);
        assert_eq!(secs, crate::breath::SESSION_MIN_SECS);
        assert_eq!(p, crate::breath::BreathPattern::clamp_from_raw(99, 4, 4, 4));
    }

    #[test]
    fn keep_screen_awake_from_db_defaults_off_for_every_mode() {
        let db = Database::open_in_memory().unwrap();
        assert!(!keep_screen_awake_from_db(&db, SessionMode::Timer));
        assert!(!keep_screen_awake_from_db(&db, SessionMode::Guided));
        assert!(!keep_screen_awake_from_db(&db, SessionMode::BoxBreath));
    }

    #[test]
    fn keep_screen_awake_from_db_reflects_per_mode_persistence() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting(keep_screen_awake_key_for_mode(SessionMode::Timer), "true").unwrap();
        assert!(keep_screen_awake_from_db(&db, SessionMode::Timer));
        assert!(!keep_screen_awake_from_db(&db, SessionMode::Guided));
    }

    #[test]
    fn label_uuid_keys_are_distinct_per_mode() {
        assert_three_distinct([
            label_uuid_key_for_mode(SessionMode::Timer),
            label_uuid_key_for_mode(SessionMode::Guided),
            label_uuid_key_for_mode(SessionMode::BoxBreath),
        ]);
    }

    /// Different knobs must NOT share a key family — e.g. the
    /// stopwatch and signal-mode keys for Timer must not be the same
    /// string. Defends against a copy-paste bug that would have one
    /// toggle silently shadow another.
    #[test]
    fn knob_families_do_not_collide_within_a_mode() {
        let signal = signal_mode_key_for_mode(SessionMode::Timer);
        let awake = keep_screen_awake_key_for_mode(SessionMode::Timer);
        let stopwatch = stopwatch_key_for_mode(SessionMode::Timer);
        let label_active = label_active_key_for_mode(SessionMode::Timer);
        let label_uuid = label_uuid_key_for_mode(SessionMode::Timer);
        let all = [signal, awake, stopwatch, label_active, label_uuid];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j], "key collision within Timer mode");
            }
        }
    }

    #[test]
    fn default_label_uuid_for_mode_picks_per_mode_seed() {
        use crate::seeds::{
            DEFAULT_BREATHING_LABEL_UUID, DEFAULT_GUIDED_LABEL_UUID, DEFAULT_TIMER_LABEL_UUID,
        };
        assert_eq!(
            default_label_uuid_for_mode(SessionMode::Timer),
            DEFAULT_TIMER_LABEL_UUID
        );
        assert_eq!(
            default_label_uuid_for_mode(SessionMode::BoxBreath),
            DEFAULT_BREATHING_LABEL_UUID
        );
        assert_eq!(
            default_label_uuid_for_mode(SessionMode::Guided),
            DEFAULT_GUIDED_LABEL_UUID
        );
    }
}
