//! Box-breath / 4-7-8 / arbitrary 4-phase breath patterns.
//!
//! Single source of truth shared by every shell. Phase variants and
//! struct shape match the GTK shell's prior `Pattern` (the canonical
//! visual reference); the API exposes a unified
//! `phase_at(elapsed) -> PhaseInfo` so callers don't have to chain
//! `phase_at` + `phase_progress` + `phase_total` + `phase_remaining`.
//!
//! Zero-length phases are skipped — a 4-7-8-0 pattern with no final
//! hold cycles through three active phases without ever returning
//! `Phase::HoldOut`.

use crate::timer::Stopwatch;
use std::time::Duration;

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
            Phase::In => 0,
            Phase::HoldIn => 1,
            Phase::Out => 2,
            Phase::HoldOut => 3,
        }
    }
}

/// Translatable key for the running-page phase label ("Breathe in",
/// "Hold", "Breathe out"). The shell maps each variant to gettext;
/// `Hold` covers both `HoldIn` and `HoldOut` because the user-facing
/// prompt is the same in both cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseRunningLabelKey {
    BreatheIn,
    Hold,
    BreatheOut,
}

pub fn phase_running_label_key(phase: Phase) -> PhaseRunningLabelKey {
    match phase {
        Phase::In => PhaseRunningLabelKey::BreatheIn,
        Phase::HoldIn | Phase::HoldOut => PhaseRunningLabelKey::Hold,
        Phase::Out => PhaseRunningLabelKey::BreatheOut,
    }
}

/// Position of the moving dot on the Box-Breath running page's
/// square perimeter, given the current phase + intra-phase progress
/// `t ∈ [0, 1]`. `pad` is the inset (top/left offset of the square
/// inside the drawing area) and `side` is the square's side length.
/// Phases run clockwise from the bottom-left corner so inhalation
/// reads as upward motion and exhalation as downward — reinforcing
/// the breath metaphor. The shell consumes `(x, y)` directly into
/// its native drawing call.
pub fn perimeter_point(phase: Phase, t: f64, pad: f64, side: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    match phase {
        // In: left edge, bottom → top.
        Phase::In => (pad, pad + side * (1.0 - t)),
        // HoldIn: top edge, left → right.
        Phase::HoldIn => (pad + side * t, pad),
        // Out: right edge, top → bottom.
        Phase::Out => (pad + side, pad + side * t),
        // HoldOut: bottom edge, right → left.
        Phase::HoldOut => (pad + side * (1.0 - t), pad + side),
    }
}

/// Maximum per-phase duration (seconds). Mirrors the GTK editor's
/// SpinRow upper bound and the runtime sampler's expectation that
/// no single phase runs longer than ~20s.
pub const PHASE_MAX_SECS: u32 = 20;

/// Minimum legal cycle length. Below this, `phase_at` panics on a
/// zero-length cycle — defence in depth against a 0-0-0-0 pattern
/// reaching the running view.
pub const MIN_CYCLE_SECS: u32 = 1;

/// Lower bound on a Box-Breath session duration, in seconds.
/// Anything less and the cycle-aligned rounding produces an
/// empty session.
pub const SESSION_MIN_SECS: u32 = 60;

/// Upper bound on a Box-Breath session duration, in seconds.
/// 23 hours 59 minutes — the inherited SpinRow cap.
pub const SESSION_MAX_SECS: u32 = 23 * 3600 + 59 * 60;

/// Clamp a raw session-duration request into the supported range.
pub fn clamp_session_secs(secs: u32) -> u32 {
    secs.clamp(SESSION_MIN_SECS, SESSION_MAX_SECS)
}

/// Four-phase breath pattern. Durations are seconds (matches the
/// GTK shell's settings-key persistence and editor SpinRow ranges).
/// `0` for any field skips that phase entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BreathPattern {
    pub in_secs: u32,
    pub hold_in: u32,
    pub out_secs: u32,
    pub hold_out: u32,
}

/// Where a given moment lands inside one breath cycle. All four
/// fields together give the running page everything it needs:
/// the active phase to label, how far through it we are (for the
/// dot's perimeter position or a progress bar), the phase's full
/// duration (denominator for "M / N" displays), and the remaining
/// time in this phase (the big seconds digit overlaid on the
/// square in the GTK box-breath running page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseInfo {
    pub phase: Phase,
    pub elapsed_in_phase: Duration,
    pub total: Duration,
    pub remaining: Duration,
}

impl BreathPattern {
    pub fn from_durations(in_secs: u32, hold_in: u32, out_secs: u32, hold_out: u32) -> Self {
        Self { in_secs, hold_in, out_secs, hold_out }
    }

    /// Box breath: 4-4-4-4.
    pub fn box_breath() -> Self {
        Self { in_secs: 4, hold_in: 4, out_secs: 4, hold_out: 4 }
    }

    /// 4-7-8 (Weil) — inhale 4, hold 7, exhale 8, no final hold.
    pub fn four_seven_eight() -> Self {
        Self { in_secs: 4, hold_in: 7, out_secs: 8, hold_out: 0 }
    }

    /// Total cycle length. Sum of the four phase durations.
    pub fn cycle(&self) -> Duration {
        Duration::from_secs(
            (self.in_secs + self.hold_in + self.out_secs + self.hold_out) as u64,
        )
    }

    /// Round a requested session duration UP to the next full cycle
    /// boundary, so a Box-Breath session always ends on an exhale
    /// or hold-out boundary rather than mid-phase. `cycle().max(1)`
    /// is the divisor so degenerate zero-cycle patterns (which can
    /// only happen mid-edit) don't divide by zero.
    pub fn cycle_aligned_target_secs(&self, raw_secs: u64) -> u64 {
        let cycle = self.cycle().as_secs().max(1);
        raw_secs.div_ceil(cycle) * cycle
    }

    /// Clamp + min-policy applied to raw per-phase values (e.g. read
    /// from settings, typed into a spinner). Active phases
    /// (`in`/`out`) require at least 1 second; hold phases may be
    /// zero (4-7-8 with `hold_out=0` is valid). All four cap at
    /// `PHASE_MAX_SECS`.
    pub fn clamp_from_raw(in_secs: u32, hold_in: u32, out_secs: u32, hold_out: u32) -> Self {
        Self {
            in_secs: in_secs.clamp(1, PHASE_MAX_SECS),
            hold_in: hold_in.clamp(0, PHASE_MAX_SECS),
            out_secs: out_secs.clamp(1, PHASE_MAX_SECS),
            hold_out: hold_out.clamp(0, PHASE_MAX_SECS),
        }
    }

    /// Minimum allowed value for phase `index` (0=In, 1=HoldIn,
    /// 2=Out, 3=HoldOut). Active phases need 1 second to keep the
    /// cycle non-degenerate; hold phases may be zero.
    pub fn phase_min_secs(index: u8) -> u32 {
        match index {
            0 | 2 => 1,
            _ => 0,
        }
    }

    pub fn duration_for(&self, phase: Phase) -> Duration {
        let secs = match phase {
            Phase::In => self.in_secs,
            Phase::HoldIn => self.hold_in,
            Phase::Out => self.out_secs,
            Phase::HoldOut => self.hold_out,
        };
        Duration::from_secs(secs as u64)
    }

    /// The four phases paired with their durations, in cycle order.
    /// Zero-duration phases stay in the array so callers can iterate
    /// positionally (`phase_at` is what skips them).
    pub fn phases(&self) -> [(Phase, u32); 4] {
        [
            (Phase::In, self.in_secs),
            (Phase::HoldIn, self.hold_in),
            (Phase::Out, self.out_secs),
            (Phase::HoldOut, self.hold_out),
        ]
    }

    /// The active phase + how far into it we are at `elapsed`. Wraps
    /// past one cycle (so a session running ten minutes through a
    /// 16-second box-breath cycle keeps cycling). Zero-length phases
    /// are skipped — a 4-7-8-0 pattern's `phase_at` never returns
    /// `Phase::HoldOut`.
    ///
    /// Panics in debug builds if the pattern has a zero cycle (every
    /// phase is 0). That's user-input validation territory; the
    /// editor / setup screen must enforce a non-zero cycle before
    /// starting a session.
    pub fn phase_at(&self, elapsed: Duration) -> PhaseInfo {
        let cycle = self.cycle();
        debug_assert!(!cycle.is_zero(), "phase_at: zero-length cycle");

        let cycle_nanos = cycle.as_nanos();
        let t_nanos = elapsed.as_nanos() % cycle_nanos;

        let mut acc: u128 = 0;
        for (phase, dur_secs) in self.phases() {
            if dur_secs == 0 {
                continue;
            }
            let dur_nanos: u128 = (dur_secs as u128) * 1_000_000_000;
            let next = acc + dur_nanos;
            if t_nanos < next {
                let into = t_nanos - acc;
                return PhaseInfo {
                    phase,
                    elapsed_in_phase: Duration::from_nanos(into as u64),
                    total: Duration::from_nanos(dur_nanos as u64),
                    remaining: Duration::from_nanos((dur_nanos - into) as u64),
                };
            }
            acc = next;
        }
        // After `% cycle_nanos` the loop above always returns. Defensive
        // tail in case of a pattern with zero-duration trailing phases —
        // return the last non-zero phase at its end.
        let last = self
            .phases()
            .into_iter()
            .rev()
            .find(|(_, d)| *d > 0)
            .expect("phase_at: zero-length cycle");
        let dur = Duration::from_secs(last.1 as u64);
        PhaseInfo {
            phase: last.0,
            elapsed_in_phase: dur,
            total: dur,
            remaining: Duration::ZERO,
        }
    }

    /// The phase that represents the end of a cycle — the one we
    /// align session completion to. Prefers `HoldOut` if non-zero,
    /// then `Out`, then `HoldIn`, then `In` (matches 4-7-8-0 style
    /// patterns where the trailing hold is skipped).
    pub fn last_phase(&self) -> Phase {
        if self.hold_out > 0 {
            Phase::HoldOut
        } else if self.out_secs > 0 {
            Phase::Out
        } else if self.hold_in > 0 {
            Phase::HoldIn
        } else {
            Phase::In
        }
    }
}

/// Pattern + Stopwatch wrapper. Convenient when a shell wants
/// "session" semantics — pause/resume freezes/continues the
/// elapsed time without the caller plumbing the `now` arithmetic.
pub struct BreathSession {
    pattern: BreathPattern,
    stopwatch: Stopwatch,
}

impl BreathSession {
    pub fn new(pattern: BreathPattern, stopwatch: Stopwatch) -> Self {
        Self { pattern, stopwatch }
    }

    pub fn phase_info(&self, now: Duration) -> PhaseInfo {
        self.pattern.phase_at(self.stopwatch.elapsed(now))
    }

    pub fn pause(self, now: Duration) -> Self {
        let Self { pattern, stopwatch } = self;
        Self {
            pattern,
            stopwatch: stopwatch.paused_at(now),
        }
    }

    pub fn resume(self, now: Duration) -> Self {
        let Self { pattern, stopwatch } = self;
        Self {
            pattern,
            stopwatch: stopwatch.resumed_at(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_pattern() -> BreathPattern {
        BreathPattern::box_breath()
    }

    fn four_seven_eight() -> BreathPattern {
        BreathPattern::four_seven_eight()
    }

    // ── Invariants ──────────────────────────────────────────────────

    #[test]
    fn cycle_aligned_target_rounds_up_to_next_full_cycle() {
        let p = box_pattern(); // cycle = 16s
        // 60s requested → next multiple of 16 ≥ 60 is 64.
        assert_eq!(p.cycle_aligned_target_secs(60), 64);
        // 0s edge case (mid-edit) → still produces something legal.
        assert_eq!(p.cycle_aligned_target_secs(0), 0);
        // Exact multiple stays put.
        assert_eq!(p.cycle_aligned_target_secs(128), 128);
    }

    #[test]
    fn cycle_aligned_target_survives_degenerate_zero_cycle() {
        let p = BreathPattern { in_secs: 0, hold_in: 0, out_secs: 0, hold_out: 0 };
        // Divisor max(1) keeps this from dividing by zero.
        assert_eq!(p.cycle_aligned_target_secs(60), 60);
    }

    #[test]
    fn clamp_from_raw_enforces_active_phase_minimum() {
        // in/out cannot be 0; hold phases can.
        let p = BreathPattern::clamp_from_raw(0, 0, 0, 0);
        assert_eq!(p, BreathPattern { in_secs: 1, hold_in: 0, out_secs: 1, hold_out: 0 });
    }

    #[test]
    fn clamp_from_raw_caps_at_phase_max() {
        let p = BreathPattern::clamp_from_raw(99, 99, 99, 99);
        assert_eq!(
            p,
            BreathPattern {
                in_secs: PHASE_MAX_SECS,
                hold_in: PHASE_MAX_SECS,
                out_secs: PHASE_MAX_SECS,
                hold_out: PHASE_MAX_SECS,
            }
        );
    }

    #[test]
    fn phase_min_secs_per_phase() {
        // Active phases (in / out) need 1; hold phases may be zero.
        assert_eq!(BreathPattern::phase_min_secs(0), 1);
        assert_eq!(BreathPattern::phase_min_secs(1), 0);
        assert_eq!(BreathPattern::phase_min_secs(2), 1);
        assert_eq!(BreathPattern::phase_min_secs(3), 0);
    }

    #[test]
    fn clamp_session_secs_floors_at_minimum() {
        assert_eq!(clamp_session_secs(0), SESSION_MIN_SECS);
        assert_eq!(clamp_session_secs(59), SESSION_MIN_SECS);
        assert_eq!(clamp_session_secs(60), 60);
    }

    #[test]
    fn clamp_session_secs_caps_at_maximum() {
        assert_eq!(clamp_session_secs(u32::MAX), SESSION_MAX_SECS);
        assert_eq!(clamp_session_secs(SESSION_MAX_SECS), SESSION_MAX_SECS);
        assert_eq!(clamp_session_secs(SESSION_MAX_SECS + 1), SESSION_MAX_SECS);
    }

    // ── BreathPattern: cycle / from_durations ──────────────────────────

    #[test]
    fn cycle_sums_all_four_phases() {
        assert_eq!(box_pattern().cycle(), Duration::from_secs(16));
        assert_eq!(four_seven_eight().cycle(), Duration::from_secs(19));
    }

    #[test]
    fn from_durations_assembles_an_arbitrary_pattern() {
        let p = BreathPattern::from_durations(5, 5, 5, 5);
        assert_eq!(p.cycle(), Duration::from_secs(20));
    }

    #[test]
    fn duration_for_returns_per_phase_duration() {
        let p = four_seven_eight();
        assert_eq!(p.duration_for(Phase::In), Duration::from_secs(4));
        assert_eq!(p.duration_for(Phase::HoldIn), Duration::from_secs(7));
        assert_eq!(p.duration_for(Phase::Out), Duration::from_secs(8));
        assert_eq!(p.duration_for(Phase::HoldOut), Duration::ZERO);
    }

    // ── phase_at: box pattern ──────────────────────────────────────────

    #[test]
    fn phase_at_start_is_in_at_zero() {
        let info = box_pattern().phase_at(Duration::ZERO);
        assert_eq!(info.phase, Phase::In);
        assert_eq!(info.elapsed_in_phase, Duration::ZERO);
        assert_eq!(info.total, Duration::from_secs(4));
        assert_eq!(info.remaining, Duration::from_secs(4));
    }

    #[test]
    fn phase_at_boundary_picks_next_phase() {
        // Exactly-at-boundary: 4.0 into a 4-second inhale is the start of
        // hold-in, not the end of inhale — the boundary belongs to the
        // next phase.
        let info = box_pattern().phase_at(Duration::from_secs(4));
        assert_eq!(info.phase, Phase::HoldIn);
        assert_eq!(info.elapsed_in_phase, Duration::ZERO);
        assert_eq!(info.remaining, Duration::from_secs(4));

        assert_eq!(box_pattern().phase_at(Duration::from_secs(8)).phase, Phase::Out);
        assert_eq!(box_pattern().phase_at(Duration::from_secs(12)).phase, Phase::HoldOut);
    }

    #[test]
    fn phase_at_fractional_within_phase() {
        let info = box_pattern().phase_at(Duration::from_millis(2_500));
        assert_eq!(info.phase, Phase::In);
        assert_eq!(info.elapsed_in_phase, Duration::from_millis(2_500));
        assert_eq!(info.total, Duration::from_secs(4));
        assert_eq!(info.remaining, Duration::from_millis(1_500));
    }

    #[test]
    fn phase_at_wraps_past_cycle_end() {
        // 17.5 s into a 16 s cycle = 1.5 s into the next inhale.
        let info = box_pattern().phase_at(Duration::from_millis(17_500));
        assert_eq!(info.phase, Phase::In);
        assert_eq!(info.elapsed_in_phase, Duration::from_millis(1_500));
        assert_eq!(info.remaining, Duration::from_millis(2_500));
    }

    #[test]
    fn phase_at_wraps_far_past_cycle() {
        // 100 cycles + 5 s offset = mid-HoldIn.
        let info = box_pattern().phase_at(Duration::from_secs(16 * 100 + 5));
        assert_eq!(info.phase, Phase::HoldIn);
        assert_eq!(info.elapsed_in_phase, Duration::from_secs(1));
    }

    // ── phase_at: 4-7-8-0 (skipped final hold) ─────────────────────────

    #[test]
    fn phase_at_skips_zero_duration_phase() {
        // 4-7-8-0: after In (0..4) + HoldIn (4..11) + Out (11..19) the
        // cycle wraps back to In — the 0-second HoldOut never appears.
        let info = four_seven_eight().phase_at(Duration::from_secs(12));
        assert_eq!(info.phase, Phase::Out);
        assert_eq!(info.elapsed_in_phase, Duration::from_secs(1));
        assert_eq!(info.total, Duration::from_secs(8));

        // At the boundary where HoldOut would start (t=19), wrap back to In at 0.
        let info = four_seven_eight().phase_at(Duration::from_secs(19));
        assert_eq!(info.phase, Phase::In);
        assert_eq!(info.elapsed_in_phase, Duration::ZERO);
    }

    #[test]
    fn phase_at_4_7_8_through_full_cycle() {
        let p = four_seven_eight();
        assert_eq!(p.phase_at(Duration::ZERO).phase, Phase::In);
        assert_eq!(p.phase_at(Duration::from_secs(4)).phase, Phase::HoldIn);
        assert_eq!(p.phase_at(Duration::from_secs(11)).phase, Phase::Out);
    }

    // ── PhaseInfo: total + remaining invariants ───────────────────────

    #[test]
    fn phase_info_total_equals_phase_duration() {
        let p = four_seven_eight();
        assert_eq!(p.phase_at(Duration::from_secs(0)).total, Duration::from_secs(4));
        assert_eq!(p.phase_at(Duration::from_secs(4)).total, Duration::from_secs(7));
        assert_eq!(p.phase_at(Duration::from_secs(11)).total, Duration::from_secs(8));
    }

    #[test]
    fn phase_info_remaining_decreases_through_phase() {
        let info_a = box_pattern().phase_at(Duration::from_secs(1));
        let info_b = box_pattern().phase_at(Duration::from_secs(3));
        assert_eq!(info_a.remaining, Duration::from_secs(3));
        assert_eq!(info_b.remaining, Duration::from_secs(1));
    }

    #[test]
    fn phase_info_elapsed_plus_remaining_equals_total() {
        let info = box_pattern().phase_at(Duration::from_millis(500));
        assert_eq!(info.elapsed_in_phase + info.remaining, info.total);
    }

    #[test]
    fn phase_info_remaining_resets_at_phase_boundary() {
        // t=4 starts HoldIn — remaining should be the full HoldIn duration.
        let info = box_pattern().phase_at(Duration::from_secs(4));
        assert_eq!(info.remaining, Duration::from_secs(4));
    }

    // ── last_phase ─────────────────────────────────────────────────────

    #[test]
    fn last_phase_prefers_trailing_nonzero() {
        assert_eq!(box_pattern().last_phase(), Phase::HoldOut);
        assert_eq!(four_seven_eight().last_phase(), Phase::Out);
        let only_in = BreathPattern::from_durations(5, 0, 0, 0);
        assert_eq!(only_in.last_phase(), Phase::In);
    }

    // ── BreathSession ──────────────────────────────────────────────────

    #[test]
    fn breath_session_phase_info_via_stopwatch_elapsed() {
        let session = BreathSession::new(
            box_pattern(),
            Stopwatch::started_at(Duration::from_secs(100)),
        );
        // 4 s after start → HoldIn at 0 elapsed-into-phase.
        let info = session.phase_info(Duration::from_secs(104));
        assert_eq!(info.phase, Phase::HoldIn);
        assert_eq!(info.elapsed_in_phase, Duration::ZERO);
    }

    #[test]
    fn breath_session_pause_then_resume_freezes_then_continues() {
        let session = BreathSession::new(
            box_pattern(),
            Stopwatch::started_at(Duration::from_secs(100)),
        )
        .pause(Duration::from_secs(102)) // 2 s into Inhale, paused
        .resume(Duration::from_secs(200)); // 98 s of wall time skipped
        // Active elapsed at t=210 = 2 s + (210-200) = 12 s. Box breath:
        // In [0-4), HoldIn [4-8), Out [8-12), HoldOut [12-16).
        let info = session.phase_info(Duration::from_secs(210));
        assert_eq!(info.phase, Phase::HoldOut);
    }

    // ── Phase::index ───────────────────────────────────────────────────

    #[test]
    fn phase_index_matches_cycle_order() {
        assert_eq!(Phase::In.index(), 0);
        assert_eq!(Phase::HoldIn.index(), 1);
        assert_eq!(Phase::Out.index(), 2);
        assert_eq!(Phase::HoldOut.index(), 3);
    }

    // ── perimeter_point ─────────────────────────────────────────────

    #[test]
    fn perimeter_point_starts_each_phase_at_the_correct_corner() {
        // Square at (pad=10, side=100). Corners:
        //   (10, 10)   top-left
        //   (110, 10)  top-right
        //   (110, 110) bottom-right
        //   (10, 110)  bottom-left
        // In starts at bottom-left, ends at top-left.
        assert_eq!(perimeter_point(Phase::In, 0.0, 10.0, 100.0), (10.0, 110.0));
        // HoldIn starts at top-left.
        assert_eq!(perimeter_point(Phase::HoldIn, 0.0, 10.0, 100.0), (10.0, 10.0));
        // Out starts at top-right.
        assert_eq!(perimeter_point(Phase::Out, 0.0, 10.0, 100.0), (110.0, 10.0));
        // HoldOut starts at bottom-right.
        assert_eq!(perimeter_point(Phase::HoldOut, 0.0, 10.0, 100.0), (110.0, 110.0));
    }

    #[test]
    fn perimeter_point_ends_each_phase_at_the_next_corner() {
        // In ends at top-left.
        assert_eq!(perimeter_point(Phase::In, 1.0, 10.0, 100.0), (10.0, 10.0));
        // HoldIn ends at top-right.
        assert_eq!(perimeter_point(Phase::HoldIn, 1.0, 10.0, 100.0), (110.0, 10.0));
        // Out ends at bottom-right.
        assert_eq!(perimeter_point(Phase::Out, 1.0, 10.0, 100.0), (110.0, 110.0));
        // HoldOut ends at bottom-left.
        assert_eq!(perimeter_point(Phase::HoldOut, 1.0, 10.0, 100.0), (10.0, 110.0));
    }

    #[test]
    fn perimeter_point_clamps_progress_outside_unit_range() {
        // t below 0 clamps to phase start.
        assert_eq!(
            perimeter_point(Phase::In, -0.5, 10.0, 100.0),
            perimeter_point(Phase::In, 0.0, 10.0, 100.0),
        );
        // t above 1 clamps to phase end.
        assert_eq!(
            perimeter_point(Phase::HoldIn, 2.0, 10.0, 100.0),
            perimeter_point(Phase::HoldIn, 1.0, 10.0, 100.0),
        );
    }
}
