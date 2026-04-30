//! Rate-limit backoff state for the bulk-push retry loop.
//!
//! Before each PUT attempt the caller checks `wait_until_now()` and
//! sleeps for the returned duration if any. On a 429 they call
//! `note_429()` which advances the "next attempt allowed at" instant
//! according to the server's `Retry-After` (or an exponential
//! fallback). On success they call `note_success()` which clears the
//! consecutive-429 counter so a future 429 starts at a fresh small
//! delay rather than at whatever exponent a previous burst left.
//!
//! Pure data + pure functions: no IO, no clock dependency in the
//! tests (which pass an explicit `now`), so the unit tests are
//! deterministic.

use std::time::{Duration, Instant};

/// Maximum exponential backoff in seconds when the server didn't send a
/// Retry-After header. 30 s is small enough that a transient burst
/// recovers within a normal sync attempt, large enough that we stop
/// hammering a server that's actively asking us to back off.
pub const MAX_BACKOFF_SECS: u64 = 30;

/// Per-push backoff state. Owned by the retry loop in
/// `put_with_rate_limit_retry`; one fresh instance per push attempt.
#[derive(Debug, Default)]
pub struct BackoffState {
    /// Earliest instant at which the next PUT attempt is allowed.
    /// `None` means no backoff active — proceed immediately.
    retry_at: Option<Instant>,
    /// How many consecutive 429s have been recorded since the last
    /// successful PUT. Drives the exponential-backoff calculation
    /// when the server doesn't supply a Retry-After.
    consecutive: u32,
}

impl BackoffState {
    pub fn new() -> Self { Self::default() }

    /// How long the caller should sleep before its next attempt, given
    /// the current `now`. `None` means proceed immediately.
    /// Parameterised on `now` so unit tests don't need a real clock.
    pub fn wait_for(&self, now: Instant) -> Option<Duration> {
        self.retry_at.and_then(|t| t.checked_duration_since(now))
    }

    /// Convenience wrapper that uses `Instant::now()` — what production
    /// callers want. Tests prefer `wait_for(now)` for determinism.
    pub fn wait_until_now(&self) -> Option<Duration> {
        self.wait_for(Instant::now())
    }

    /// Record a 429. Updates `retry_at` to the later of (a) any current
    /// backoff window and (b) the server-suggested or exponentially-
    /// computed delay from `now`. Taking the later means a fresh 429
    /// from a relaxed Retry-After can never shorten a stricter window
    /// already in effect.
    ///
    /// `retry_after_secs` is the value of the `Retry-After` header the
    /// server sent (None if missing). When supplied it's authoritative
    /// — exponential backoff only kicks in when the server is silent.
    pub fn note_429_at(&mut self, now: Instant, retry_after_secs: Option<u64>) {
        self.consecutive = self.consecutive.saturating_add(1);
        let secs = retry_after_secs.unwrap_or_else(|| {
            // Exponential: 1, 2, 4, 8, 16, 32, … capped at MAX_BACKOFF_SECS.
            // `consecutive` is at least 1 here (we just incremented).
            let shift = (self.consecutive - 1).min(6);
            (1u64 << shift).min(MAX_BACKOFF_SECS)
        });
        let candidate = now + Duration::from_secs(secs);
        if self.retry_at.map_or(true, |cur| candidate > cur) {
            self.retry_at = Some(candidate);
        }
    }

    /// Test/production convenience: same as `note_429_at` with `Instant::now()`.
    pub fn note_429(&mut self, retry_after_secs: Option<u64>) {
        self.note_429_at(Instant::now(), retry_after_secs);
    }

    /// Record a successful PUT. Clears the consecutive-429 counter so
    /// the next 429 (if any) starts a fresh exponential ramp from 1 s.
    /// Does NOT clear `retry_at`: an in-flight backoff window is still
    /// honored (the server explicitly asked for it). Future 429s will
    /// compute their candidate from `now` again, so the window expires
    /// naturally as `Instant::now()` overtakes it.
    pub fn note_success(&mut self) {
        self.consecutive = 0;
    }

    /// Test introspection: how many consecutive 429s seen.
    #[cfg(test)]
    pub fn consecutive(&self) -> u32 { self.consecutive }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t0() -> Instant { Instant::now() }

    #[test]
    fn fresh_state_imposes_no_wait() {
        // A freshly-constructed state imposes no delay — push starts
        // at full speed and only throttles when the server pushes back.
        let s = BackoffState::new();
        assert_eq!(s.wait_for(t0()), None);
    }

    #[test]
    fn note_429_with_explicit_retry_after_is_authoritative() {
        // The server's instruction wins. If it says 17 s, that's what
        // we sleep — not our exponential schedule.
        let now = t0();
        let mut s = BackoffState::new();
        s.note_429_at(now, Some(17));
        let waited = s.wait_for(now).expect("expected a backoff window");
        // The window starts at exactly `now + 17 s`; we passed `now`
        // again as the query time, so the gap is the full 17 s.
        assert_eq!(waited.as_secs(), 17);
    }

    #[test]
    fn note_429_without_retry_after_uses_one_second_for_first_hit() {
        // Exponential schedule: 1, 2, 4, 8, ... — first 429 starts at 1 s.
        let now = t0();
        let mut s = BackoffState::new();
        s.note_429_at(now, None);
        let waited = s.wait_for(now).unwrap();
        assert_eq!(waited.as_secs(), 1);
    }

    #[test]
    fn note_429_without_retry_after_grows_exponentially() {
        // 1, 2, 4 — confirms doubling for the first three 429s.
        // Successive calls compound the counter, but each new window
        // is computed from the current `now`, so the assertion is on
        // the size of the JUST-set window.
        let now = t0();
        let mut s = BackoffState::new();
        s.note_429_at(now, None);
        assert_eq!(s.wait_for(now).unwrap().as_secs(), 1, "1st 429 → 1 s");
        s.note_429_at(now, None);
        assert_eq!(s.wait_for(now).unwrap().as_secs(), 2, "2nd 429 → 2 s");
        s.note_429_at(now, None);
        assert_eq!(s.wait_for(now).unwrap().as_secs(), 4, "3rd 429 → 4 s");
    }

    #[test]
    fn note_429_caps_exponential_backoff_at_the_constant() {
        // 12 consecutive 429s would compute 2^11 = 2048 s without the
        // cap. The cap exists because a backoff longer than the user's
        // patience is just a failed sync; better to hit a ceiling and
        // surface "rate limited, try later" via the status indicator.
        let now = t0();
        let mut s = BackoffState::new();
        for _ in 0..12 { s.note_429_at(now, None); }
        let secs = s.wait_for(now).unwrap().as_secs();
        assert!(secs <= MAX_BACKOFF_SECS,
            "backoff must cap at {MAX_BACKOFF_SECS}s, got {secs}");
    }

    #[test]
    fn note_429_does_not_shrink_an_existing_longer_window() {
        // First 429 has Retry-After: 60. A subsequent 429 a moment
        // later has Retry-After: 5. The original 60 s window must
        // survive — a relaxed Retry-After can never shorten a
        // stricter window already in effect.
        let now = t0();
        let mut s = BackoffState::new();
        s.note_429_at(now, Some(60));
        let later = now + Duration::from_secs(2);
        s.note_429_at(later, Some(5));
        // The first window ends at now+60 = later+58 s. The 5-second
        // candidate would put us at later+5 s; we must keep the
        // longer window — wait queried at `later` should be close to
        // 58 s, not 5 s.
        let waited = s.wait_for(later).unwrap();
        assert!(waited.as_secs() >= 50,
            "must keep the longer window; got only {}s", waited.as_secs());
    }

    #[test]
    fn note_429_extends_a_shorter_existing_window() {
        // Inverse of the previous test: a longer second 429 must
        // extend the window. Otherwise a server tightening its limits
        // mid-batch would be ignored.
        let now = t0();
        let mut s = BackoffState::new();
        s.note_429_at(now, Some(2));
        s.note_429_at(now, Some(20));
        assert_eq!(s.wait_for(now).unwrap().as_secs(), 20);
    }

    #[test]
    fn note_success_resets_consecutive_counter() {
        // After enough successes the next 429 should start back at 1 s,
        // not at where the previous burst left off. This is what stops
        // a permanently-rate-limited day from blowing past the cap on
        // every subsequent transient.
        let now = t0();
        let mut s = BackoffState::new();
        s.note_429_at(now, None);
        s.note_429_at(now, None);
        assert_eq!(s.consecutive(), 2);
        s.note_success();
        assert_eq!(s.consecutive(), 0);
        // After success, the next 429's exponential is back to 1 s.
        let later = now + Duration::from_secs(60);
        s.note_429_at(later, None);
        assert_eq!(s.wait_for(later).unwrap().as_secs(), 1,
            "consecutive reset must restart exponential from 1 s");
    }

    #[test]
    fn note_success_does_not_clear_an_in_flight_window() {
        // The server told us "wait 30 s". A success ack (e.g. from an
        // unrelated request that slipped past the throttle) must NOT
        // grant permission to ignore the server's explicit Retry-After
        // — that's a `note_success`-resets-counter-only invariant.
        let now = t0();
        let mut s = BackoffState::new();
        s.note_429_at(now, Some(30));
        s.note_success();
        let waited = s.wait_for(now);
        assert!(waited.is_some(),
            "in-flight window must persist through a success ack");
    }

    #[test]
    fn wait_for_returns_none_after_window_expires() {
        // The expiry path: the server-suggested time has elapsed.
        // Workers should resume normal operation.
        let now = t0();
        let mut s = BackoffState::new();
        s.note_429_at(now, Some(2));
        let after_window = now + Duration::from_secs(5);
        assert_eq!(s.wait_for(after_window), None,
            "expired window must yield no further wait");
    }

    #[test]
    fn note_429_consecutive_is_saturating_under_pathological_volume() {
        // u32 overflow defense — if a server returned millions of
        // 429s, `consecutive += 1` must not panic. The saturating add
        // means it just sticks at u32::MAX.
        let now = t0();
        let mut s = BackoffState::new();
        s.consecutive = u32::MAX - 1;
        s.note_429_at(now, None);
        s.note_429_at(now, None);
        assert_eq!(s.consecutive(), u32::MAX);
    }
}
