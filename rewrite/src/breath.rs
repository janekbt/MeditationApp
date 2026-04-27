use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub enum Phase {
    Inhale,
}

pub struct BreathPattern;

impl BreathPattern {
    pub fn box_breath() -> Self {
        Self
    }

    pub fn phase_at(&self, _elapsed: Duration) -> Phase {
        Phase::Inhale
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
}
