//! Filesystem-layout conventions shared across shells.
//!
//! Each shell decides its own *data root* (gtk uses
//! `glib::user_data_dir()`, Android uses `Context::getFilesDir()`),
//! but the per-subdirectory + per-filename naming under that root is
//! a stable cross-shell convention. Pinning the strings here
//! prevents drift on the relative paths the sync orchestrator uses
//! to identify peer files.

/// App's own subdir under the data root (e.g.
/// `~/.local/share/meditate/` on a Linux desktop). Every other
/// constant in this module sits inside this folder.
pub const APP_SUBDIR: &str = "meditate";

/// Custom bell-sound files (the user's imports + the local cache
/// of synced peer files). Filenames are `{uuid}.{ext}` per
/// `BellSound::extension()`. Lives under `APP_SUBDIR`.
pub const SOUNDS_SUBDIR: &str = "sounds";

/// Guided-meditation audio files. Filenames are `{uuid}.ogg` (the
/// importer transcodes to ogg for non-passthrough formats). Lives
/// under `APP_SUBDIR`.
pub const GUIDED_SUBDIR: &str = "guided";

/// SQLite database filename — pinned so an Android port using
/// `Room` or a manual `SQLiteOpenHelper` lands on the same file
/// name a desktop sync drop-in would.
pub const DB_FILENAME: &str = "meditate.db";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_stable_strings() {
        // Tripwire: any change to these constants is a wire-format
        // change that requires migration handling on every shell.
        // Failing this test forces an explicit ack.
        assert_eq!(APP_SUBDIR, "meditate");
        assert_eq!(SOUNDS_SUBDIR, "sounds");
        assert_eq!(GUIDED_SUBDIR, "guided");
        assert_eq!(DB_FILENAME, "meditate.db");
    }
}
