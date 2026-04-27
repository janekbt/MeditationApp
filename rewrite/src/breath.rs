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
}
