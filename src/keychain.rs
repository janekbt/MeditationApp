//! Storage of the Nextcloud app-password in the user's freedesktop
//! Secret Service (gnome-keyring on most desktops, KWallet, etc.) via
//! the `oo7` crate.
//!
//! `oo7` is async-only and requires a tokio reactor for D-Bus traffic,
//! so we own a small persistent tokio runtime. The public functions
//! here are synchronous: each `block_on`s the async work on the global
//! runtime. Keychain operations finish in tens of milliseconds typically,
//! so blocking the calling thread (often the GLib main thread, on
//! settings save / app startup) is acceptable. The first call pays a
//! one-time D-Bus connection setup cost.
//!
//! The schema attribute identifies items as ours so co-resident apps
//! using libsecret don't see them in their search results, and a
//! `url` + `username` attribute pair forms the natural primary key
//! (one credential per (server, user)). If the user reconfigures
//! against a new URL the old credential stays in the keyring until
//! they explicitly delete it — we don't try to garbage-collect.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::OnceLock;

const SCHEMA: &str = "io.github.janekbt.Meditate.NextcloudSync";
const ATTR_URL: &str = "url";
const ATTR_USER: &str = "username";

#[derive(Debug)]
pub enum KeychainError {
    /// Underlying oo7 / Secret Service failure: keyring locked, D-Bus
    /// down, no Secret Service running, etc. The string is opaque —
    /// callers should surface it verbatim in the diagnostics log
    /// rather than try to act on it.
    Backend(String),
}

impl fmt::Display for KeychainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(s) => write!(f, "keychain: {s}"),
        }
    }
}

impl Error for KeychainError {}

impl From<oo7::Error> for KeychainError {
    fn from(e: oo7::Error) -> Self {
        Self::Backend(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, KeychainError>;

/// Global tokio runtime driving oo7's D-Bus traffic. Lazily initialised
/// on first call. Multi-threaded so concurrent calls from different
/// threads (settings UI on the GLib main thread + sync worker on its
/// own thread) don't queue serially.
fn rt() -> &'static tokio::runtime::Runtime {
    static R: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    R.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("create tokio runtime for oo7 (keychain)")
    })
}

fn attributes<'a>(url: &'a str, username: &'a str) -> HashMap<&'a str, &'a str> {
    let mut attrs = HashMap::with_capacity(3);
    attrs.insert(oo7::XDG_SCHEMA_ATTRIBUTE, SCHEMA);
    attrs.insert(ATTR_URL, url);
    attrs.insert(ATTR_USER, username);
    attrs
}

/// Save a password for the given (url, username) pair. Replaces any
/// existing item that matches the same attributes — settings flow's
/// "save" button trusts whatever is currently typed in the dialog.
pub fn store_password(url: &str, username: &str, password: &str) -> Result<()> {
    rt().block_on(async {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        let label = format!("Meditate sync — {username} on {url}");
        keyring.create_item(
            &label,
            &attributes(url, username),
            password.as_bytes(),
            true,
        ).await?;
        Ok(())
    })
}

/// Look up a password for (url, username). `Ok(None)` when no matching
/// item is in the keyring; `Err(...)` only on infrastructure failure
/// (Secret Service unreachable, decryption failed, …).
pub fn read_password(url: &str, username: &str) -> Result<Option<String>> {
    rt().block_on(async {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        let items = keyring.search_items(&attributes(url, username)).await?;
        let Some(item) = items.into_iter().next() else { return Ok(None); };
        let secret = item.secret().await?;
        // The secret bytes are user input — a Nextcloud app-password
        // generated on the security panel — so they're ASCII / UTF-8.
        // Defensive against malformed UTF-8 just in case.
        let pw = String::from_utf8(secret.to_vec())
            .map_err(|e| KeychainError::Backend(format!("password is not valid UTF-8: {e}")))?;
        Ok(Some(pw))
    })
}

/// Remove the password for (url, username). No-op if the item doesn't
/// exist; the underlying `keyring.delete()` already handles "no match"
/// silently.
pub fn delete_password(url: &str, username: &str) -> Result<()> {
    rt().block_on(async {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        keyring.delete(&attributes(url, username)).await?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    //! These tests cover only the bits of `keychain` that are pure
    //! Rust — the actual store/read/delete go through D-Bus and need
    //! a running Secret Service, which is the `keychain_smoke` binary's
    //! domain. Pinning the schema constant and attribute shape here
    //! makes sure a future rename of `ATTR_URL` (or similar) trips a
    //! cheap test rather than silently invalidating every existing
    //! keyring item the user has stored.

    use super::*;

    #[test]
    fn schema_constant_namespaces_to_our_app_id() {
        // The schema attribute is what distinguishes our items in the
        // user's keyring from every other app's. Renaming it would
        // orphan all existing stored passwords.
        assert_eq!(SCHEMA, "io.github.janekbt.Meditate.NextcloudSync");
    }

    #[test]
    fn attributes_includes_schema_url_and_username() {
        let attrs = attributes("https://nc.example/", "janek");
        assert_eq!(attrs.get(oo7::XDG_SCHEMA_ATTRIBUTE), Some(&SCHEMA));
        assert_eq!(attrs.get(ATTR_URL), Some(&"https://nc.example/"));
        assert_eq!(attrs.get(ATTR_USER), Some(&"janek"));
        assert_eq!(attrs.len(), 3, "no stray attributes leak into search keys");
    }

    #[test]
    fn attribute_keys_are_stable_strings() {
        // If we ever accidentally rename ATTR_URL or ATTR_USER, every
        // already-stored item becomes orphaned (search by new key
        // misses, search by old key finds them). Pin the wire names.
        assert_eq!(ATTR_URL, "url");
        assert_eq!(ATTR_USER, "username");
    }

    #[test]
    fn keychain_error_display_is_descriptive() {
        // The error message gets logged to diag.log when sync fails;
        // the user reads it to figure out what's wrong.
        let err = KeychainError::Backend("D-Bus connection refused".to_string());
        assert_eq!(err.to_string(), "keychain: D-Bus connection refused");
    }
}
