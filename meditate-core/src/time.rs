//! Time helpers shared between shells.
//!
//! Currently a single function — `boot_time_now()` — that returns
//! suspend-resilient monotonic time. Future siblings (unix↔ISO
//! conversions in item 14) land here too.

/// Suspend-resilient monotonic time. Linux's `std::time::Instant` uses
/// CLOCK_MONOTONIC, which freezes during system suspend — a 30s suspend
/// in the middle of a session would silently lose 30s of countdown.
/// CLOCK_BOOTTIME counts time including suspend, which is what a meditation
/// timer wants: real wall-clock progress regardless of OS power state.
///
/// On Android, Rust's `Instant::now()` already uses CLOCK_BOOTTIME natively
/// since 1.79 — but a single shared helper that callers depend on means
/// every shell stays consistent regardless of which platform's std happens
/// to do the right thing.
pub fn boot_time_now() -> std::time::Duration {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    debug_assert_eq!(rc, 0, "clock_gettime(CLOCK_BOOTTIME) failed");
    std::time::Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_time_now_is_monotonically_non_decreasing() {
        // Two consecutive reads must not go backwards. We can't assert
        // anything about the absolute value (varies by host), only the
        // monotonic invariant that drives every caller.
        let a = boot_time_now();
        let b = boot_time_now();
        assert!(b >= a, "second read {b:?} preceded first {a:?}");
    }

    #[test]
    fn boot_time_now_advances_across_a_real_sleep() {
        let before = boot_time_now();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let after = boot_time_now();
        assert!(
            after.saturating_sub(before) >= std::time::Duration::from_millis(5),
            "did not advance across a 10ms sleep: before={before:?} after={after:?}"
        );
    }
}
