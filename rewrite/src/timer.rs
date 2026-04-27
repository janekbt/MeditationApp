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

pub enum Stopwatch {
    Running {
        running_since: Duration,
        prior_accumulated: Duration,
    },
    Paused {
        accumulated: Duration,
    },
}

impl Stopwatch {
    pub fn started_at(now: Duration) -> Self {
        Self::Running {
            running_since: now,
            prior_accumulated: Duration::ZERO,
        }
    }

    pub fn paused_at(self, now: Duration) -> Self {
        Self::Paused {
            accumulated: self.elapsed(now),
        }
    }

    pub fn resumed_at(self, now: Duration) -> Self {
        Self::Running {
            running_since: now,
            prior_accumulated: self.elapsed(now),
        }
    }

    pub fn elapsed(&self, now: Duration) -> Duration {
        match self {
            Self::Running {
                running_since,
                prior_accumulated,
            } => *prior_accumulated + now.saturating_sub(*running_since),
            Self::Paused { accumulated } => *accumulated,
        }
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

    #[test]
    fn stopwatch_elapsed_grows_with_now() {
        let stopwatch = Stopwatch::started_at(Duration::from_secs(100));
        assert_eq!(stopwatch.elapsed(Duration::from_secs(150)), Duration::from_secs(50));
    }

    #[test]
    fn paused_stopwatch_does_not_accumulate_elapsed_after_pause() {
        let stopwatch = Stopwatch::started_at(Duration::from_secs(100))
            .paused_at(Duration::from_secs(110));
        assert_eq!(stopwatch.elapsed(Duration::from_secs(200)), Duration::from_secs(10));
    }

    #[test]
    fn resumed_stopwatch_continues_from_accumulated_elapsed() {
        let stopwatch = Stopwatch::started_at(Duration::from_secs(100))
            .paused_at(Duration::from_secs(110))
            .resumed_at(Duration::from_secs(200));
        assert_eq!(stopwatch.elapsed(Duration::from_secs(210)), Duration::from_secs(20));
    }
}
