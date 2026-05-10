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

use crate::db::SessionMode;

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
