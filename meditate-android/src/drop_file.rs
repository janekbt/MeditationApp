//! Drop files: how the Kotlin side hands a result (a pick, an import
//! outcome, a widget tap) to the tick loop. Kotlin writes them
//! atomically (MeditateDropFile.kt); the Rust side reads and removes
//! them here, once.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;

/// Read `<data>/meditate/<name>` and remove it. Removed even when the
/// caller then fails to parse it, so a bad file can't repeat every
/// tick. `None` when nothing is pending.
pub fn take(app: &AndroidApp, name: &str) -> Option<String> {
    let path = app.internal_data_path()?.join("meditate").join(name);
    let raw = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    Some(raw)
}
