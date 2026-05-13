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
        .map(|v| parse_bool(&v))
        .unwrap_or(default)
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
