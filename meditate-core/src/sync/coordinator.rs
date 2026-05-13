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

    /// Free the in-flight slot at the end of the drain loop. Returns
    /// `true` when the slot was truly released; `false` when a
    /// re-trigger arrived in the narrow window between
    /// `should_run_again_after_pass()` reading false and this call
    /// — in that case `release` re-takes the slot and the caller
    /// MUST run another pass instead of exiting.
    ///
    /// Why the double-check: without it, the sequence
    ///
    ///   worker: should_run_again_after_pass() → false  (exit signal)
    ///   external: request() → re_trigger=true, in_flight still true → AlreadyRunning
    ///   worker: in_flight=false                       (slot released)
    ///
    /// strands `re_trigger=true` with no observing worker. The next
    /// sync would silently never run unless another trigger arrives.
    /// The CAS dance below — store false, recheck re_trigger,
    /// retake the slot if needed — closes that window.
    #[must_use = "caller must run another pass when release returns false"]
    pub fn release(&self) -> bool {
        self.in_flight.store(false, Ordering::SeqCst);
        if !self.re_trigger.load(Ordering::SeqCst) {
            return true;
        }
        // A re-trigger snuck in between should_run_again_after_pass()
        // and release(). Try to re-take the slot. If somebody else
        // already grabbed it (their own request() raced and saw
        // in_flight=false from our store), they'll run the pass —
        // exit cleanly. Otherwise we own the slot again and the
        // caller's loop must continue.
        if self.in_flight.swap(true, Ordering::SeqCst) {
            // Slot taken by someone else; let them run, we exit.
            true
        } else {
            // We re-took the slot; loop must run another pass.
            false
        }
    }

    /// Bail variant of `release` for the caller's error-path:
    /// take-the-slot then immediately fail to start the worker
    /// (e.g. DB path unavailable). Unconditionally clears
    /// `in_flight` — no re-trigger handling because no worker
    /// was actually running, so there's no drain loop to continue.
    pub fn abort(&self) {
        self.in_flight.store(false, Ordering::SeqCst);
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
        assert!(c.release(), "no re-trigger → release returns true");
        assert!(!c.is_in_flight());
    }

    #[test]
    fn release_frees_slot_for_next_request() {
        let c = SyncCoordinator::new();
        let _ = c.request();
        c.start_pass();
        assert!(c.release());
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
            if !c.should_run_again_after_pass() && c.release() {
                break;
            }
        }
        assert_eq!(passes, 2, "loop must run exactly two passes");
        assert!(!c.is_in_flight());
    }

    // ── Drop-trigger race: the reason `release` does a double-check ─────────

    #[test]
    fn release_catches_trigger_that_lands_between_check_and_release() {
        // Synthesise the race deterministically (without threads):
        //   1. Worker reads should_run_again_after_pass → false.
        //   2. External thread calls request() — sets re_trigger,
        //      sees in_flight still true, returns AlreadyRunning.
        //   3. Worker calls release().
        // Pre-fix: release just cleared in_flight and the re-trigger
        // was stranded. Now: release re-takes the slot and returns
        // false, signalling the loop to run another pass.
        let c = SyncCoordinator::new();
        assert_eq!(c.request(), CoordinatorAction::Spawn);
        c.start_pass();
        assert!(!c.should_run_again_after_pass(),
            "no trigger during pass yet");
        // Trigger lands in the narrow window.
        assert_eq!(c.request(), CoordinatorAction::AlreadyRunning);
        // Worker calls release without re-checking the flag itself.
        assert!(!c.release(),
            "release must signal 'loop again' when re-trigger snuck in");
        assert!(c.is_in_flight(),
            "slot stays held — the worker continues without anyone else taking it");
    }

    #[test]
    fn release_no_op_when_in_flight_was_already_taken_by_a_racing_request() {
        // The other branch of the same race: by the time release
        // tries to retake the slot, a concurrent request has
        // already done so (request raced with our store of false
        // and saw it before we re-took). The racing request now
        // owns the slot; release exits cleanly, returns true.
        let c = SyncCoordinator::new();
        assert_eq!(c.request(), CoordinatorAction::Spawn);
        c.start_pass();
        assert!(!c.should_run_again_after_pass());
        // Manually walk the release dance in two parts so a request
        // can land between them, simulating the racier window.
        c.in_flight.store(false, Ordering::SeqCst);
        // Someone else takes the slot before we check re_trigger.
        assert_eq!(c.request(), CoordinatorAction::Spawn);
        // Now finish the release dance — re_trigger is set (from
        // request) but in_flight is also true (request took it).
        if !c.re_trigger.load(Ordering::SeqCst) {
            panic!("test setup: request must have set re_trigger");
        }
        let retook = !c.in_flight.swap(true, Ordering::SeqCst);
        assert!(!retook, "we did NOT retake the slot — racing request did");
        // That's the path inside release where it returns true (exit).
    }

}
