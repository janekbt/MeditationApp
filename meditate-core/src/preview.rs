//! Single-slot preview state machine: toggle / supersede / auto-revert.
//!
//! The vibration-pattern chooser uses this today. The bell-sound
//! chooser ships its own thinner mono-slot equivalent (`src/sound.rs`
//! `play_preview` / `stop_preview` over `PREVIEW_MEDIA`); the typed
//! state machine here is what both choosers want to converge on so
//! the Android shell can share one preview protocol. Both contexts
//! follow the same rules:
//!
//! - Tap on an inactive row/button → start playback, flip icon to Stop.
//! - Tap on the active row/button → stop playback, flip back to Play.
//! - Tap on a different row while another is playing → stop the prior,
//!   start the new (single playback channel; the shell's playback
//!   transport supersedes via its own same-app rule — feedbackd for
//!   vibration, the GTK media-file pipeline for sound).
//! - After `duration_ms` the natural-end timeout fires and reverts
//!   the icon IF the same play is still active — invalidated by any
//!   later user action via a monotonic generation counter.
//!
//! `PreviewToggle` owns the state, exposes `request(id)` returning a
//! typed [`PreviewAction`], and provides `timer_should_revert(gen)`
//! for the auto-revert callback. The shell does the actual playback
//! (`PatternPlayback` / `MediaPlayer`) and the icon swap.
//!
//! Lives in its own module so both `meditate_core::vibration` and
//! `meditate_core::sound` can re-export it without one importing the
//! other.

#[derive(Debug, Clone, Default)]
pub struct PreviewToggle {
    /// Identifier of the currently-playing preview, or `None` when
    /// nothing is playing.
    active_id: Option<String>,
    /// Monotonic generation counter; bumped on every state change so
    /// a stale auto-revert timer from an earlier play knows to no-op.
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewAction {
    /// Cancel the currently-playing preview (if any) and start a
    /// new one identified by `id`. The `generation` token is what
    /// the shell passes to `timer_should_revert` when the natural-
    /// end timer fires for this play.
    StopAndStart { id: String, generation: u64 },
    /// Cancel the currently-playing preview without starting a new
    /// one. Returned when the user tapped the active row's button
    /// (toggle off) or called `stop()` directly.
    StopOnly,
    /// Nothing was playing — no-op for the shell. Returned by
    /// `stop()` when there's nothing to stop.
    NoOp,
}

impl PreviewToggle {
    /// Fresh toggle with nothing playing. Equivalent to `default()`;
    /// kept named so call sites read as construction rather than as a
    /// trait method invocation.
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of the currently-playing preview, or `None` if the
    /// toggle is idle. Used by the chooser to highlight the active
    /// row and by the auto-revert timer to confirm playback is still
    /// the one that started it.
    pub fn active_id(&self) -> Option<&str> {
        self.active_id.as_deref()
    }

    /// Whether any preview is in flight. Mirror of
    /// `active_id().is_some()`, exposed for readability.
    pub fn is_playing(&self) -> bool {
        self.active_id.is_some()
    }

    /// User tapped Play for `id`. Returns the action to take:
    ///   - tapping the active id → `StopOnly`
    ///   - tapping any other id (or starting fresh) → `StopAndStart`
    pub fn request(&mut self, id: &str) -> PreviewAction {
        self.generation = self.generation.wrapping_add(1);
        if self.active_id.as_deref() == Some(id) {
            self.active_id = None;
            PreviewAction::StopOnly
        } else {
            self.active_id = Some(id.to_string());
            PreviewAction::StopAndStart {
                id: id.to_string(),
                generation: self.generation,
            }
        }
    }

    /// Stop the active preview without starting anything new.
    pub fn stop(&mut self) -> PreviewAction {
        if self.active_id.is_some() {
            self.active_id = None;
            self.generation = self.generation.wrapping_add(1);
            PreviewAction::StopOnly
        } else {
            PreviewAction::NoOp
        }
    }

    /// Called from the natural-end auto-revert timer with the
    /// generation token returned by `request`. Returns `true` if
    /// the icon should flip back to Play (this generation is still
    /// the active one); `false` when the user has since stopped /
    /// restarted, in which case the shell's timer callback no-ops.
    /// On `true`, clears the active id so a subsequent tap starts
    /// fresh.
    pub fn timer_should_revert(&mut self, generation: u64) -> bool {
        if self.generation == generation && self.active_id.is_some() {
            self.active_id = None;
            self.generation = self.generation.wrapping_add(1);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_macros::assert_matches;

    #[test]
    fn preview_starts_fresh_on_first_request() {
        let mut p = PreviewToggle::new();
        let action = p.request("pattern-a");
        assert_matches!(
            &action,
            PreviewAction::StopAndStart { id, .. } => assert_eq!(id, "pattern-a"),
        );
        assert!(p.is_playing());
        assert_eq!(p.active_id(), Some("pattern-a"));
    }

    #[test]
    fn preview_request_on_active_id_stops() {
        let mut p = PreviewToggle::new();
        let _ = p.request("pattern-a");
        let action = p.request("pattern-a");
        assert_eq!(action, PreviewAction::StopOnly);
        assert!(!p.is_playing());
    }

    #[test]
    fn preview_request_on_different_id_supersedes() {
        let mut p = PreviewToggle::new();
        let _ = p.request("pattern-a");
        let action = p.request("pattern-b");
        assert_matches!(
            &action,
            PreviewAction::StopAndStart { id, .. } => assert_eq!(id, "pattern-b"),
        );
        assert_eq!(p.active_id(), Some("pattern-b"));
    }

    #[test]
    fn preview_stop_without_active_is_noop() {
        let mut p = PreviewToggle::new();
        assert_eq!(p.stop(), PreviewAction::NoOp);
    }

    #[test]
    fn preview_stop_with_active_returns_stop_only() {
        let mut p = PreviewToggle::new();
        let _ = p.request("pattern-a");
        assert_eq!(p.stop(), PreviewAction::StopOnly);
        assert!(!p.is_playing());
    }

    #[test]
    fn timer_should_revert_on_matching_generation() {
        let mut p = PreviewToggle::new();
        let gen = match p.request("pattern-a") {
            PreviewAction::StopAndStart { generation, .. } => generation,
            _ => panic!("expected StopAndStart"),
        };
        assert!(p.timer_should_revert(gen));
        assert!(!p.is_playing(), "revert clears active");
    }

    #[test]
    fn timer_should_revert_noops_when_user_already_stopped() {
        let mut p = PreviewToggle::new();
        let gen = match p.request("pattern-a") {
            PreviewAction::StopAndStart { generation, .. } => generation,
            _ => panic!("expected StopAndStart"),
        };
        let _ = p.stop();
        assert!(!p.timer_should_revert(gen), "stale timer must no-op");
    }

    #[test]
    fn timer_should_revert_noops_when_user_switched_to_other_pattern() {
        let mut p = PreviewToggle::new();
        let gen_a = match p.request("pattern-a") {
            PreviewAction::StopAndStart { generation, .. } => generation,
            _ => panic!("expected StopAndStart"),
        };
        let _ = p.request("pattern-b");
        assert!(!p.timer_should_revert(gen_a), "old gen no-ops");
        assert_eq!(p.active_id(), Some("pattern-b"));
    }
}
