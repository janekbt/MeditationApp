//! Pure phase arithmetic for Box-Breath mode.
//!
//! No GTK, no I/O — takes a `Pattern` (four phase durations in seconds)
//! and a total elapsed time, returns which phase the user is in plus
//! how far through that phase they are. Zero-length phases are skipped
//! (so a 4-7-8-0 pattern with no final hold works identically to a
//! "normal" three-phase cycle).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    In,
    HoldIn,
    Out,
    HoldOut,
}

impl Phase {
    pub fn index(self) -> usize {
        match self {
            Phase::In      => 0,
            Phase::HoldIn  => 1,
            Phase::Out     => 2,
            Phase::HoldOut => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pattern {
    pub in_secs:   u32,
    pub hold_in:   u32,
    pub out_secs:  u32,
    pub hold_out:  u32,
}

impl Pattern {
    pub fn cycle_secs(&self) -> u32 {
        self.in_secs + self.hold_in + self.out_secs + self.hold_out
    }

    /// Returns the four phases in order paired with their durations.
    /// Zero-duration phases stay in the list so callers can still iterate
    /// them positionally if needed — `phase_at` is what skips them.
    pub fn phases(&self) -> [(Phase, u32); 4] {
        [
            (Phase::In,      self.in_secs),
            (Phase::HoldIn,  self.hold_in),
            (Phase::Out,     self.out_secs),
            (Phase::HoldOut, self.hold_out),
        ]
    }
}

/// Where a given moment lands inside one breath cycle.
///
/// `phase_elapsed` is seconds into the active phase (0.0 ≤ x < phase_total).
/// `phase_total` is the active phase's duration (always > 0 — zero phases
/// are skipped rather than returned).
///
/// Panics if the pattern has no non-zero phases — that's user-input validation
/// territory and should be caught at the setup screen (require cycle ≥ 1s).
pub fn phase_at(pattern: &Pattern, elapsed_in_cycle: f64) -> (Phase, f64, u32) {
    let cycle = pattern.cycle_secs();
    debug_assert!(cycle > 0, "phase_at: pattern has zero-length cycle");

    // Wrap negative and past-cycle values into [0, cycle). Using rem_euclid
    // on f64 keeps the sign correct for pauses that resume with slight drift.
    let t = elapsed_in_cycle.rem_euclid(cycle as f64);

    let mut acc = 0.0;
    for (phase, dur) in pattern.phases() {
        if dur == 0 {
            continue;
        }
        let next = acc + dur as f64;
        if t < next {
            return (phase, t - acc, dur);
        }
        acc = next;
    }
    // Floating-point edge case: t == cycle exactly (after rem_euclid this
    // shouldn't happen, but guard anyway). Return the last non-zero phase at
    // its end.
    let last = pattern.phases()
        .into_iter()
        .rev()
        .find(|(_, d)| *d > 0)
        .expect("phase_at: pattern has zero-length cycle");
    (last.0, last.1 as f64, last.1)
}

/// The phase that represents the end of a cycle — the one we align session
/// completion to. Prefers `HoldOut` if it's non-zero, otherwise `Out`
/// (matches 4-7-8-0 style patterns where the final hold is skipped).
pub fn last_phase(pattern: &Pattern) -> Phase {
    if pattern.hold_out > 0 {
        Phase::HoldOut
    } else if pattern.out_secs > 0 {
        Phase::Out
    } else if pattern.hold_in > 0 {
        Phase::HoldIn
    } else {
        Phase::In
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_pattern() -> Pattern {
        Pattern { in_secs: 4, hold_in: 4, out_secs: 4, hold_out: 4 }
    }
    fn four_seven_eight() -> Pattern {
        // 4-7-8-0 — no final hold.
        Pattern { in_secs: 4, hold_in: 7, out_secs: 8, hold_out: 0 }
    }

    #[test]
    fn cycle_secs_sums_all_phases() {
        assert_eq!(box_pattern().cycle_secs(), 16);
        assert_eq!(four_seven_eight().cycle_secs(), 19);
    }

    #[test]
    fn phase_at_start_is_inhale_zero() {
        let (p, elapsed, total) = phase_at(&box_pattern(), 0.0);
        assert_eq!(p, Phase::In);
        assert!((elapsed - 0.0).abs() < 1e-9);
        assert_eq!(total, 4);
    }

    #[test]
    fn phase_at_boundaries_pick_next_phase() {
        // Exactly-at-boundary: 4.0 into a 4-second inhale is the start of
        // hold-in, not the end of inhale — the boundary belongs to the next
        // phase.
        let (p, elapsed, _) = phase_at(&box_pattern(), 4.0);
        assert_eq!(p, Phase::HoldIn);
        assert!((elapsed - 0.0).abs() < 1e-9);

        let (p, _, _) = phase_at(&box_pattern(), 8.0);
        assert_eq!(p, Phase::Out);

        let (p, _, _) = phase_at(&box_pattern(), 12.0);
        assert_eq!(p, Phase::HoldOut);
    }

    #[test]
    fn phase_at_fractional_within_phase() {
        let (p, elapsed, total) = phase_at(&box_pattern(), 2.5);
        assert_eq!(p, Phase::In);
        assert!((elapsed - 2.5).abs() < 1e-9);
        assert_eq!(total, 4);

        let (p, elapsed, _) = phase_at(&box_pattern(), 9.75);
        assert_eq!(p, Phase::Out);
        assert!((elapsed - 1.75).abs() < 1e-9);
    }

    #[test]
    fn phase_at_wraps_past_cycle_end() {
        // 17.5s into a 16-s cycle = 1.5s into the next inhale.
        let (p, elapsed, _) = phase_at(&box_pattern(), 17.5);
        assert_eq!(p, Phase::In);
        assert!((elapsed - 1.5).abs() < 1e-9);
    }

    #[test]
    fn phase_at_skips_zero_duration_phase() {
        // 4-7-8-0: after In (0..4) + HoldIn (4..11) + Out (11..19) the
        // cycle wraps back to In. At t=12, we should be 1s into Out.
        let (p, elapsed, _) = phase_at(&four_seven_eight(), 12.0);
        assert_eq!(p, Phase::Out);
        assert!((elapsed - 1.0).abs() < 1e-9);

        // At the boundary where the hold_out would start (t=19), we wrap
        // back to In at 0.
        let (p, elapsed, _) = phase_at(&four_seven_eight(), 19.0);
        assert_eq!(p, Phase::In);
        assert!((elapsed - 0.0).abs() < 1e-9);
    }

    #[test]
    fn last_phase_prefers_trailing_nonzero() {
        assert_eq!(last_phase(&box_pattern()), Phase::HoldOut);
        assert_eq!(last_phase(&four_seven_eight()), Phase::Out);
        let only_in = Pattern { in_secs: 5, hold_in: 0, out_secs: 0, hold_out: 0 };
        assert_eq!(last_phase(&only_in), Phase::In);
    }
}
