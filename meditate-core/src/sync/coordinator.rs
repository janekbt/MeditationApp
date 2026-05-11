//! Sync-attempt coordinator.
//!
//! Encodes the "at most one sync in flight; bursts collapse to
//! exactly one follow-up" rule the gtk shell currently runs inline
//! in `MeditateApplication::trigger_sync`. Pure `AtomicBool`
//! choreography over a sync-callable closure — the shell owns the
//! threading model (it uses `std::thread::spawn`; Android will use
//! coroutines or a WorkManager job), this module owns the ordering
//! invariants:
//!
//! 1. `re_trigger.store(true)` BEFORE `in_flight.swap(true)` so a
//!    sync finishing mid-call still picks the new request up via
//!    its completion check.
//! 2. The drain loop clears `re_trigger` BEFORE each pass so a
//!    trigger arriving DURING the pass survives to schedule another
//!    pass.
//! 3. The in-flight slot is released BEFORE the post-completion
//!    callback so a trigger that lands while the callback is
//!    running can spawn a fresh worker.

use std::sync::atomic::{AtomicBool, Ordering};

/// Two-flag coordinator: `in_flight` blocks concurrent passes,
/// `re_trigger` carries the "another request arrived" signal
/// across the running pass.
#[derive(Debug, Default)]
pub struct SyncCoordinator {
    in_flight: AtomicBool,
    re_trigger: AtomicBool,
}

/// What the shell should do in response to a `request()` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorAction {
    /// Caller now owns the in-flight slot — spawn a worker that
    /// runs `pass()` in a loop driven by `release_and_check_retrigger`.
    Spawn,
    /// Another pass is already running. The re-trigger flag is set
    /// so that pass will run another iteration when it finishes;
    /// caller has nothing to do.
    AlreadyRunning,
}

impl SyncCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a sync is currently in flight. Used by the shell's
    /// status indicator to render the spinner.
    pub fn is_in_flight(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// User (or system) wants a sync to run. Returns `Spawn` when
    /// the caller now owns the in-flight slot, `AlreadyRunning`
    /// otherwise. Sets the re-trigger flag BEFORE attempting to
    /// take the slot so a sync finishing concurrently still picks
    /// us up.
    pub fn request(&self) -> CoordinatorAction {
        self.re_trigger.store(true, Ordering::SeqCst);
        if self.in_flight.swap(true, Ordering::SeqCst) {
            CoordinatorAction::AlreadyRunning
        } else {
            CoordinatorAction::Spawn
        }
    }

    /// Called at the START of each pass to clear the re-trigger
    /// flag. A trigger arriving DURING the pass sets it back,
    /// which `should_run_again_after_pass` then observes.
    pub fn start_pass(&self) {
        self.re_trigger.store(false, Ordering::SeqCst);
    }

    /// Called at the END of each pass to check whether a fresh
    /// trigger landed in the meantime. `true` → run another pass
    /// (loop body continues); `false` → release the in-flight slot
    /// and return.
    pub fn should_run_again_after_pass(&self) -> bool {
        self.re_trigger.load(Ordering::SeqCst)
    }

    /// Called once after the drain loop exits to free the in-flight
    /// slot. Returns whether a trigger landed between the last
    /// `start_pass` and this call — `true` means the caller's
    /// loop missed a re-trigger and should not actually release;
    /// in practice the shell ignores this return because the loop
    /// already checks `should_run_again_after_pass` immediately
    /// before exit, but it's exposed for symmetry.
    pub fn release(&self) {
        self.in_flight.store(false, Ordering::SeqCst);
    }

    /// Bail variant of `release` for the caller's error-path:
    /// take-the-slot then immediately fail to start the worker
    /// (e.g. DB path unavailable). Same effect as `release` but
    /// named to make the call-site intent explicit.
    pub fn abort(&self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_request_spawns_and_takes_slot() {
        let c = SyncCoordinator::new();
        assert_eq!(c.request(), CoordinatorAction::Spawn);
        assert!(c.is_in_flight());
    }

    #[test]
    fn second_concurrent_request_says_already_running() {
        let c = SyncCoordinator::new();
        assert_eq!(c.request(), CoordinatorAction::Spawn);
        assert_eq!(c.request(), CoordinatorAction::AlreadyRunning);
    }

    #[test]
    fn re_trigger_survives_pass_boundary() {
        let c = SyncCoordinator::new();
        let _ = c.request();
        c.start_pass();
        // Trigger arrives during the pass.
        let _ = c.request();
        assert!(c.should_run_again_after_pass(), "re-trigger must be observable");
    }

    #[test]
    fn loop_drains_when_no_re_trigger_during_pass() {
        let c = SyncCoordinator::new();
        let _ = c.request();
        c.start_pass();
        // No re-trigger arrived.
        assert!(!c.should_run_again_after_pass());
        c.release();
        assert!(!c.is_in_flight());
    }

    #[test]
    fn release_frees_slot_for_next_request() {
        let c = SyncCoordinator::new();
        let _ = c.request();
        c.release();
        assert_eq!(c.request(), CoordinatorAction::Spawn);
    }

    #[test]
    fn full_drain_loop_two_passes() {
        // Pass 1 runs. During pass 1, a re-trigger arrives. Pass 2
        // runs. No re-trigger during pass 2 → loop exits.
        let c = SyncCoordinator::new();
        assert_eq!(c.request(), CoordinatorAction::Spawn);
        // Simulate worker loop.
        let mut passes = 0;
        loop {
            c.start_pass();
            passes += 1;
            // Simulate a re-trigger arriving during pass 1 only.
            if passes == 1 {
                let _ = c.request();
            }
            if !c.should_run_again_after_pass() {
                break;
            }
        }
        c.release();
        assert_eq!(passes, 2, "loop must run exactly two passes");
        assert!(!c.is_in_flight());
    }
}
