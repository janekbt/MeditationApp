use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Inhale,
    HoldAfterInhale,
    Exhale,
    HoldAfterExhale,
}

pub struct BreathPattern {
    phases: Vec<(Phase, Duration)>,
}

impl BreathPattern {
    pub fn box_breath() -> Self {
        let four_secs = Duration::from_secs(4);
        Self {
            phases: vec![
                (Phase::Inhale, four_secs),
                (Phase::HoldAfterInhale, four_secs),
                (Phase::Exhale, four_secs),
                (Phase::HoldAfterExhale, four_secs),
            ],
        }
    }

    pub fn four_seven_eight() -> Self {
        Self {
            phases: vec![
                (Phase::Inhale, Duration::from_secs(4)),
                (Phase::HoldAfterInhale, Duration::from_secs(7)),
                (Phase::Exhale, Duration::from_secs(8)),
            ],
        }
    }

    pub fn phase_at(&self, elapsed: Duration) -> Phase {
        let cycle_nanos: u128 = self.phases.iter().map(|(_, d)| d.as_nanos()).sum();
        let offset_nanos = elapsed.as_nanos() % cycle_nanos;

        let mut accumulated: u128 = 0;
        for (phase, duration) in &self.phases {
            accumulated += duration.as_nanos();
            if offset_nanos < accumulated {
                return *phase;
            }
        }
        unreachable!("phase table exhausted despite offset < cycle")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_breath_starts_in_inhale_phase() {
        let pattern = BreathPattern::box_breath();
        assert_eq!(pattern.phase_at(Duration::ZERO), Phase::Inhale);
    }

    #[test]
    fn box_breath_holds_after_inhale_at_4s() {
        let pattern = BreathPattern::box_breath();
        assert_eq!(
            pattern.phase_at(Duration::from_secs(4)),
            Phase::HoldAfterInhale
        );
    }

    #[test]
    fn box_breath_exhales_at_8s() {
        let pattern = BreathPattern::box_breath();
        assert_eq!(pattern.phase_at(Duration::from_secs(8)), Phase::Exhale);
    }

    #[test]
    fn box_breath_holds_after_exhale_at_12s() {
        let pattern = BreathPattern::box_breath();
        assert_eq!(
            pattern.phase_at(Duration::from_secs(12)),
            Phase::HoldAfterExhale
        );
    }

    #[test]
    fn box_breath_cycle_wraps_after_16s() {
        let pattern = BreathPattern::box_breath();
        assert_eq!(pattern.phase_at(Duration::from_secs(16)), Phase::Inhale);
        assert_eq!(
            pattern.phase_at(Duration::from_secs(20)),
            Phase::HoldAfterInhale
        );
    }

    #[test]
    fn four_seven_eight_cycles_through_uneven_phase_durations() {
        let pattern = BreathPattern::four_seven_eight();
        assert_eq!(pattern.phase_at(Duration::ZERO), Phase::Inhale);
        assert_eq!(
            pattern.phase_at(Duration::from_secs(4)),
            Phase::HoldAfterInhale
        );
        assert_eq!(pattern.phase_at(Duration::from_secs(11)), Phase::Exhale);
        // Cycle is 4+7+8 = 19s; wraps back to Inhale.
        assert_eq!(pattern.phase_at(Duration::from_secs(19)), Phase::Inhale);
    }
}
