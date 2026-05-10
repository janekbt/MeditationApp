//! Stateless xorshift64 RNG for the parts of `meditate-core` that
//! want deterministic, replayable randomness (bell-jitter rolls,
//! and anything else "shake this value slightly" in the future).
//!
//! Callers thread the `u64` state through successive calls rather
//! than holding a mutable global — keeps the algorithm tested,
//! makes per-session seeds explicit, and lets the shell pick its
//! own deterministic source for tests / replay.
//!
//! Not crypto. Don't use this for anything that needs to resist
//! deliberate prediction.

/// One step of xorshift64 returning a unit-uniform `f64` in
/// `[0, 1)` and the advanced state. Caller threads `state` through
/// successive calls.
///
/// A `0` seed is internally bumped to `1` — xorshift64 outputs `0`
/// forever from a `0` seed, which would be a silent footgun.
pub fn xorshift64(state: u64) -> (f64, u64) {
    let mut s = state.max(1);
    s ^= s << 13;
    s ^= s >> 7;
    s ^= s << 17;
    // Top 53 bits → f64 in [0, 1) without losing precision.
    let unit = (s >> 11) as f64 / (1u64 << 53) as f64;
    (unit, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_is_in_half_open_zero_one() {
        let mut state = 12345u64;
        for _ in 0..10_000 {
            let (u, next) = xorshift64(state);
            assert!((0.0..1.0).contains(&u), "unit out of [0,1): {u}");
            state = next;
        }
    }

    #[test]
    fn same_seed_yields_same_sequence() {
        let mut a = 42u64;
        let mut b = 42u64;
        for _ in 0..100 {
            let (ua, na) = xorshift64(a);
            let (ub, nb) = xorshift64(b);
            assert_eq!(ua, ub);
            assert_eq!(na, nb);
            a = na;
            b = nb;
        }
    }

    #[test]
    fn zero_seed_is_bumped_to_one() {
        // Without the bump, xorshift64 would stay 0 forever and
        // every draw would be 0.0. Verify the first call already
        // recovers.
        let (u, next) = xorshift64(0);
        assert!(u > 0.0, "zero seed should not produce a 0 unit");
        assert!(next != 0, "zero seed should not stay at 0");
    }

    #[test]
    fn state_does_not_collapse_over_long_runs() {
        // Rough quality check: 10k draws on a single seed shouldn't
        // produce the same unit twice in adjacent positions or
        // collapse to a constant. Not crypto — but if the algorithm
        // were broken (e.g. all-zero state), this would catch it.
        let mut state = 0xDEAD_BEEFu64;
        let mut prev = -1.0;
        let mut all_same = true;
        for _ in 0..10_000 {
            let (u, next) = xorshift64(state);
            if prev != -1.0 && (u - prev).abs() > 0.0 {
                all_same = false;
            }
            prev = u;
            state = next;
        }
        assert!(!all_same, "xorshift64 collapsed to a constant");
    }
}
