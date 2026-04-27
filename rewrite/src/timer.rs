use std::time::Duration;

pub struct CountdownTimer {
    total: Duration,
}

impl CountdownTimer {
    pub fn new(total: Duration) -> Self {
        Self { total }
    }

    pub fn remaining(&self, elapsed: Duration) -> Duration {
        self.total.saturating_sub(elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_at_start_has_full_duration_remaining() {
        let timer = CountdownTimer::new(Duration::from_secs(600));
        assert_eq!(timer.remaining(Duration::ZERO), Duration::from_secs(600));
    }

    #[test]
    fn timer_subtracts_elapsed_from_total() {
        let timer = CountdownTimer::new(Duration::from_secs(600));
        assert_eq!(
            timer.remaining(Duration::from_secs(60)),
            Duration::from_secs(540)
        );
    }

    #[test]
    fn timer_clamps_remaining_at_zero_when_elapsed_exceeds_total() {
        let timer = CountdownTimer::new(Duration::from_secs(600));
        assert_eq!(
            timer.remaining(Duration::from_secs(700)),
            Duration::ZERO
        );
    }
}
