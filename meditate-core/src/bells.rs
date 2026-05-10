//! Per-session bell schedule + per-tick decision.
//!
//! Two schedule shapes — `Interval` (recurring, jittered) and `Fixed`
//! (one-shot at a target) — wrapped in an `ActiveBell` that also
//! carries the sound / vibration UUIDs and the signal-mode the shell
//! needs to dispatch the actual playback when this bell fires.
//!
//! Pure decision logic. The shell that owns the audio device and the
//! vibration motor passes `tick(elapsed, rng)` once per session tick,
//! reads the boolean it returns, and dispatches side effects (play
//! sound, fire vibration) externally. RNG is supplied by the caller —
//! the schedule doesn't carry its own state — so the shell stays
//! free to pick its own deterministic source for tests / replay.

use crate::db::SignalMode;
use crate::format::next_interval_ring_secs;

/// One bell's per-session schedule. Built once at the moment a session
/// enters Running (after prep, if any) and mutated in place by
/// `ActiveBell::tick` thereafter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BellSchedule {
    /// Recurring interval bell — every `base_min` minutes (give or
    /// take `jitter_pct`%). `next_ring_secs` is the absolute elapsed-
    /// since-Running mark when the next ring is due.
    Interval {
        base_min: u32,
        jitter_pct: u32,
        next_ring_secs: u64,
    },
    /// One-shot bell pinned to a known offset from session start
    /// (fixed-from-start) or session end (fixed-from-end, resolved to
    /// an absolute target at build time). Flips `fired` on first
    /// match so subsequent ticks skip it.
    Fixed {
        target_secs: u64,
        fired: bool,
    },
}

/// Per-bell active state for the running tick. Sound, vibration
/// pattern, and signal-mode travel with the schedule so the shell's
/// dispatch loop has everything it needs without a second DB lookup.
#[derive(Debug, Clone)]
pub struct ActiveBell {
    pub sound: String,
    pub vibration_pattern_uuid: String,
    pub signal_mode: SignalMode,
    pub schedule: BellSchedule,
}

impl ActiveBell {
    /// Per-tick decision: did this bell's ring boundary just get crossed?
    /// Mutates the schedule so the next tick won't double-fire:
    ///   - `Interval`: rerolls `next_ring_secs` using one draw from
    ///     `rng()` for the jitter pick.
    ///   - `Fixed`: flips `fired` to true.
    /// Returns `true` if the caller should dispatch this bell's
    /// playback now. `rng()` is only called on an `Interval` fire;
    /// `Fixed` and the no-fire branches never touch it.
    pub fn tick(&mut self, elapsed_secs: u64, rng: &mut impl FnMut() -> f64) -> bool {
        match &mut self.schedule {
            BellSchedule::Interval {
                base_min,
                jitter_pct,
                next_ring_secs,
            } => {
                if elapsed_secs >= *next_ring_secs {
                    let r = rng();
                    *next_ring_secs =
                        next_interval_ring_secs(*next_ring_secs, *base_min, *jitter_pct, r);
                    true
                } else {
                    false
                }
            }
            BellSchedule::Fixed { target_secs, fired } => {
                if !*fired && elapsed_secs >= *target_secs {
                    *fired = true;
                    true
                } else {
                    false
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_bell(target_secs: u64) -> ActiveBell {
        ActiveBell {
            sound: "sound-uuid".into(),
            vibration_pattern_uuid: "pattern-uuid".into(),
            signal_mode: SignalMode::Sound,
            schedule: BellSchedule::Fixed { target_secs, fired: false },
        }
    }

    fn interval_bell(base_min: u32, jitter_pct: u32, next_ring_secs: u64) -> ActiveBell {
        ActiveBell {
            sound: "sound-uuid".into(),
            vibration_pattern_uuid: "pattern-uuid".into(),
            signal_mode: SignalMode::Sound,
            schedule: BellSchedule::Interval { base_min, jitter_pct, next_ring_secs },
        }
    }

    /// RNG fixture that returns a fixed value and counts how many times
    /// it was called. Used to prove tick consumes randomness only when
    /// it actually rerolls.
    struct CountedRng {
        value: f64,
        calls: u32,
    }
    impl CountedRng {
        fn new(value: f64) -> Self { Self { value, calls: 0 } }
        fn closure(&mut self) -> impl FnMut() -> f64 + '_ {
            || {
                self.calls += 1;
                self.value
            }
        }
    }

    // ── Fixed ──────────────────────────────────────────────────────────

    #[test]
    fn fixed_does_not_fire_before_target() {
        let mut bell = fixed_bell(60);
        let mut rng = CountedRng::new(0.5);
        assert!(!bell.tick(59, &mut rng.closure()));
        assert!(matches!(bell.schedule, BellSchedule::Fixed { fired: false, .. }));
        assert_eq!(rng.calls, 0);
    }

    #[test]
    fn fixed_fires_at_target_and_marks_fired() {
        let mut bell = fixed_bell(60);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(60, &mut rng.closure()));
        assert!(matches!(bell.schedule, BellSchedule::Fixed { fired: true, .. }));
        assert_eq!(rng.calls, 0, "fixed bells must not consume rng draws");
    }

    #[test]
    fn fixed_fires_at_first_tick_past_target() {
        // Tick rate is per-second on the gtk shell, so elapsed steps
        // by ≥ 1s; the bell fires the first tick where elapsed crosses
        // target, even if elapsed jumped past it (e.g. backgrounded
        // app catching up).
        let mut bell = fixed_bell(60);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(75, &mut rng.closure()));
    }

    #[test]
    fn fixed_does_not_re_fire_after_initial_fire() {
        let mut bell = fixed_bell(60);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(60, &mut rng.closure()));
        // Subsequent ticks past the target must NOT fire again.
        assert!(!bell.tick(61, &mut rng.closure()));
        assert!(!bell.tick(120, &mut rng.closure()));
        assert!(!bell.tick(3600, &mut rng.closure()));
        assert_eq!(rng.calls, 0);
    }

    // ── Interval ───────────────────────────────────────────────────────

    #[test]
    fn interval_does_not_fire_before_next_ring() {
        let mut bell = interval_bell(5, 0, 300);
        let mut rng = CountedRng::new(0.5);
        assert!(!bell.tick(299, &mut rng.closure()));
        // Schedule unchanged, no rng consumed.
        assert!(matches!(
            bell.schedule,
            BellSchedule::Interval { next_ring_secs: 300, .. }
        ));
        assert_eq!(rng.calls, 0);
    }

    #[test]
    fn interval_fires_at_next_ring_and_rerolls() {
        let mut bell = interval_bell(5, 0, 300);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(300, &mut rng.closure()));
        // 5 min × 60 s = 300 s base. With jitter 0% the next ring is
        // exactly 300 s after the current one — so 600 s.
        assert!(matches!(
            bell.schedule,
            BellSchedule::Interval { next_ring_secs: 600, .. }
        ));
        assert_eq!(rng.calls, 1, "interval reroll must consume exactly one rng draw");
    }

    #[test]
    fn interval_with_jitter_picks_a_value_inside_the_window() {
        // jitter_pct=20 means the next ring is in [base * 0.8, base * 1.2].
        // For base=600 s (10 min), the window is [480, 720]. Test by
        // pinning rng to 0.0 (lower bound) and 1.0 (upper bound) and
        // checking the rerolled next_ring.
        let mut bell_lo = interval_bell(10, 20, 600);
        let mut rng_lo = CountedRng::new(0.0);
        assert!(bell_lo.tick(600, &mut rng_lo.closure()));
        if let BellSchedule::Interval { next_ring_secs, .. } = bell_lo.schedule {
            assert!(
                (1080..=1200).contains(&next_ring_secs),
                "lower-rng next_ring fell outside [1080, 1200]: got {next_ring_secs}"
            );
        } else {
            panic!("schedule shape changed");
        }

        let mut bell_hi = interval_bell(10, 20, 600);
        let mut rng_hi = CountedRng::new(1.0);
        assert!(bell_hi.tick(600, &mut rng_hi.closure()));
        if let BellSchedule::Interval { next_ring_secs, .. } = bell_hi.schedule {
            assert!(
                (1200..=1320).contains(&next_ring_secs),
                "upper-rng next_ring fell outside [1200, 1320]: got {next_ring_secs}"
            );
        } else {
            panic!("schedule shape changed");
        }
    }

    #[test]
    fn interval_rerolls_only_once_per_tick_even_when_far_past() {
        // If the app was backgrounded and the tick catches up by
        // multiple ring intervals, we still fire once and reroll once.
        // Catching up is the shell's responsibility (call tick again
        // with the new elapsed), not an internal loop.
        let mut bell = interval_bell(5, 0, 300);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(900, &mut rng.closure()));
        assert_eq!(rng.calls, 1);
        // The next_ring rerolls relative to the prior value (300),
        // not the actual elapsed (900). Catch-up firing happens on
        // the next tick when the shell calls again.
        if let BellSchedule::Interval { next_ring_secs, .. } = bell.schedule {
            assert_eq!(next_ring_secs, 600);
        }
    }

    // ── Cross-shape ────────────────────────────────────────────────────

    #[test]
    fn no_fire_consumes_no_rng_draw() {
        // Across both schedule shapes: a tick that doesn't fire never
        // calls rng(). Important for shells with a single shared RNG —
        // sequential bells in the dispatch loop must not perturb each
        // other's draws based on whether earlier bells fired.
        let mut interval = interval_bell(5, 20, 300);
        let mut fixed = fixed_bell(120);
        let mut rng = CountedRng::new(0.5);
        let mut closure = rng.closure();
        assert!(!interval.tick(60, &mut closure));
        assert!(!fixed.tick(60, &mut closure));
        drop(closure);
        assert_eq!(rng.calls, 0);
    }
}
