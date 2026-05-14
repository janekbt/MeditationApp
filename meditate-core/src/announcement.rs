//! Typed announcements — completion-class events the shell surfaces
//! via toast + screen-reader live region.
//!
//! Mirrors the [`SyncSettingsError`](crate::sync::credentials::SyncSettingsError)
//! pattern: the variant set lives in core (so the gtk and Android
//! shells agree on which events are user-facing completions), and
//! each shell renders to its own native toast/live-region API. The
//! enum carries only the structured payload (counts, names) the
//! rendering needs; the shell does the gettext mapping at the call
//! site.
//!
//! Scope is intentionally narrow: only events where "I just finished
//! X for you" is the message. Error/validation toasts ("Enter a
//! password", "File too large") are out of scope — they're routed
//! through their own typed surfaces (`SyncSettingsError`,
//! domain-specific error enums) or stay as ad-hoc localized strings
//! depending on the feature.

/// User-facing completion-class events. The shell renders each
/// variant to a toast title + optional "Undo" button; an Android
/// shell can also pipe them through `AccessibilityEvent.TYPE_ANNOUNCEMENT`
/// or its native live-region equivalent. All variants are undoable
/// today; if a non-undoable variant is added later, document the
/// distinction at the variant rather than dropping the convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Announcement {
    /// In-flight session whose state was salvaged from the
    /// in-progress snapshot at the next launch (app crashed or got
    /// SIGKILL'd mid-session). The shell offers an Undo that
    /// deletes the recovered row.
    SessionRecovered { minutes: u32 },
    /// One or more sessions just deleted via the log view. The
    /// shell surfaces an Undo that restores every hidden row in the
    /// batch.
    SessionDeleted { count: u32 },
    /// User just deleted a preset; the toast offers an Undo that
    /// re-inserts the row with its original UUID.
    PresetDeleted { name: String },
    /// User overrode an existing preset's config with the current
    /// Setup view values; the toast offers an Undo that restores
    /// the prior config_json.
    PresetOverridden { name: String },
    /// User renamed / re-edited a vibration pattern; the toast
    /// offers an Undo that restores the prior name + curve.
    PatternUpdated { name: String },
    /// User deleted a vibration pattern; the toast offers an Undo
    /// that re-inserts the pattern under its original UUID so
    /// referring bells re-resolve to it.
    PatternDeleted { name: String },
}
