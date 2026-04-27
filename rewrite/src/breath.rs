use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub enum Phase {
    Inhale,
    HoldAfterInhale,
}

pub struct BreathPattern;

impl BreathPattern {
    pub fn box_breath() -> Self {
        Self
    }

    pub fn phase_at(&self, elapsed: Duration) -> Phase {
        if elapsed < Duration::from_secs(4) {
            Phase::Inhale
        } else {
            Phase::HoldAfterInhale
        }
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
}
