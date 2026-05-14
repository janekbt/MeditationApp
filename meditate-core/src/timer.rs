//! Standalone timer primitive — `Stopwatch`.
//!
//! Pause-aware elapsed-time clock used inside
//! `meditate_core::session::Session` (which owns the higher-level
//! phase/state machine). Stays a small leaf module so the
//! `serde::{Serialize, Deserialize}` derive can ride through to
//! crash-recovery persistence without dragging the Session types in.

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    #[test]
    fn running_stopwatch_round_trips_through_json() {
        let original = Stopwatch::started_at(Duration::from_secs(100));
        let json = serde_json::to_string(&original).unwrap();
        let restored: Stopwatch = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.elapsed(Duration::from_secs(110)),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn stopwatch_survives_simulated_process_restart() {
        // Shell clock at app start: monotonic boot time = 100s.
        let original = Stopwatch::started_at(Duration::from_secs(100));

        // App runs to boot time = 200s, then OS kills it.
        // (50s of meditation in the bank.)
        let saved = serde_json::to_string(&original).unwrap();

        // App relaunches later at boot time = 500s.
        // No real-world time was lost — the timer was active the whole time.
        let restored: Stopwatch = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            restored.elapsed(Duration::from_secs(500)),
            Duration::from_secs(400)
        );
    }
}
