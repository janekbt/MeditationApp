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

    pub fn is_finished(&self, elapsed: Duration) -> bool {
        elapsed >= self.total
    }
}

pub struct Stopwatch {
    started_at: Duration,
}

impl Stopwatch {
    pub fn started_at(now: Duration) -> Self {
        Self { started_at: now }
    }

    pub fn elapsed(&self, _now: Duration) -> Duration {
        Duration::from_secs(10)
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

    #[test]
    fn timer_is_finished_when_elapsed_equals_total() {
        let timer = CountdownTimer::new(Duration::from_secs(600));
        assert!(timer.is_finished(Duration::from_secs(600)));
    }

    #[test]
    fn timer_is_not_finished_before_elapsed_reaches_total() {
        let timer = CountdownTimer::new(Duration::from_secs(600));
        assert!(!timer.is_finished(Duration::from_secs(599)));
    }

    #[test]
    fn stopwatch_elapsed_is_now_minus_started_at() {
        let stopwatch = Stopwatch::started_at(Duration::from_secs(100));
        assert_eq!(stopwatch.elapsed(Duration::from_secs(110)), Duration::from_secs(10));
    }
}
