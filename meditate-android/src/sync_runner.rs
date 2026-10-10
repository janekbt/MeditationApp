//! One sync attempt: core's runner (`meditate_core::sync::runner`)
//! with the Android Keystore and this app's folders. The Keystore read
//! returns `Option<String>` with failures already logged and collapsed
//! to `None` inside the bridge, so a Keystore failure shows as a
//! missing password; the user action is the same: re-enter it.

#![cfg(target_os = "android")]

use std::path::{Path, PathBuf};

use android_activity::AndroidApp;
use meditate_core::sync::runner::RunError;
use meditate_core::sync::SyncStats;

pub fn run_sync_attempt(
    app: &AndroidApp,
    db_path: &Path,
    sounds_dir: PathBuf,
    guided_dir: PathBuf,
) -> Result<SyncStats, RunError> {
    meditate_core::sync::runner::run_attempt(db_path, sounds_dir, guided_dir, |url, username| {
        Ok(crate::keychain::read_password(app, url, username))
    })
}
