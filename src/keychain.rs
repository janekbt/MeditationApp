//! Storage of the Nextcloud app-password in the user's freedesktop
//! Secret Service via the `oo7` crate. Hybrid two-path design:
//!
//! 1. Try `oo7::Keyring::new()` first. Outside flatpak that's the
//!    Secret Service over D-Bus; inside flatpak with a working
//!    `org.freedesktop.portal.Secret` backend (e.g. xdg-desktop-portal-
//!    gnome on a regular GNOME desktop), oo7 uses a portal-derived
//!    master key to encrypt a sandboxed file keyring. Both are the
//!    "proper" paths.
//!
//! 2. If that fails specifically with the file-backend's
//!    `WeakKey(PasswordTooShort)` signal — the case where the portal
//!    is registered but no backend is wired up to actually fulfil
//!    `RetrieveSecret`, so it returns 0 bytes (Phosh on PureOS as of
//!    2026-04 — `xdg-desktop-portal-gtk` doesn't implement Secret) —
//!    fall back to a self-keyed file backend. We generate a random
//!    32-byte master key on first use, store it in our flatpak data
//!    dir alongside the encrypted keyring file. That gets us a
//!    flatpak-isolated keyring that works without depending on host
//!    packages.
//!
//! Honest threat-model note for the fallback: the master key sits
//! next to the encrypted data in the same flatpak-private directory.
//! An attacker with read access to that dir can grab both and recover
//! the password — the encryption mostly buys protection against
//! accidental exposure (backup tools that snapshot one file but not
//! the other, casual file inspection). Real at-rest protection comes
//! from filesystem permissions plus flatpak's per-app data isolation.
//! That's the same model gnome-keyring's "login" keyring uses on
//! systems where PAM doesn't unlock it (which is the case on Phosh
//! today anyway), so we're not losing real security on this platform
//! relative to the "proper" portal path.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const SCHEMA: &str = "io.github.janekbt.Meditate.NextcloudSync";
const ATTR_URL: &str = "url";
const ATTR_USER: &str = "username";

/// Filename of the encrypted keyring within our flatpak data dir.
const SELF_KEYED_KEYRING_FILENAME: &str = "nextcloud-sync.keyring";
/// Filename of the master key file alongside the keyring. Stored
/// verbatim (32 random bytes); the `Keyring::load` call uses it as
/// the HKDF input for AES-CBC + HMAC.
const SELF_KEYED_MASTER_FILENAME: &str = "nextcloud-sync.master";

#[derive(Debug)]
pub enum KeychainError {
    /// Underlying oo7 / Secret Service / file-backend failure. The
    /// string is opaque — callers should surface it verbatim in the
    /// diagnostics log rather than try to act on it.
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
    fn from(e: oo7::Error) -> Self { Self::Backend(e.to_string()) }
}

impl From<oo7::file::Error> for KeychainError {
    fn from(e: oo7::file::Error) -> Self { Self::Backend(e.to_string()) }
}

impl From<std::io::Error> for KeychainError {
    fn from(e: std::io::Error) -> Self { Self::Backend(format!("io: {e}")) }
}

pub type Result<T> = std::result::Result<T, KeychainError>;

/// Global tokio runtime driving oo7's async D-Bus / file-backend work.
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

// ── Public sync API (blocking on the global runtime) ────────────────────────

pub fn store_password(url: &str, username: &str, password: &str) -> Result<()> {
    rt().block_on(async {
        let label = format!("Meditate sync — {username} on {url}");
        let attrs = attributes(url, username);
        let bytes = password.as_bytes();
        let backend = open_chosen_backend().await?;
        match backend.create_item(&label, &attrs, bytes, true).await {
            Ok(()) => { mark_backend_ok(&backend); Ok(()) }
            Err(e) if oo7_error_is_weak_key(&e) => {
                fall_back_log(&e);
                let fb = open_self_keyed_backend().await?;
                fb.create_item(&label, &attrs, bytes, true).await?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    })
}

pub fn read_password(url: &str, username: &str) -> Result<Option<String>> {
    rt().block_on(async {
        let attrs = attributes(url, username);
        let backend = open_chosen_backend().await?;
        let result = read_one_secret(&backend, &attrs).await;
        match result {
            Ok(opt) => { mark_backend_ok(&backend); Ok(opt) }
            Err(e) if oo7_error_is_weak_key(&e) => {
                fall_back_log(&e);
                let fb = open_self_keyed_backend().await?;
                Ok(read_one_secret(&fb, &attrs).await?)
            }
            Err(e) => Err(e.into()),
        }
    })
}

pub fn delete_password(url: &str, username: &str) -> Result<()> {
    rt().block_on(async {
        let attrs = attributes(url, username);
        let backend = open_chosen_backend().await?;
        match backend.delete(&attrs).await {
            Ok(()) => { mark_backend_ok(&backend); Ok(()) }
            Err(e) if oo7_error_is_weak_key(&e) => {
                fall_back_log(&e);
                let fb = open_self_keyed_backend().await?;
                fb.delete(&attrs).await?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    })
}

/// Search + secret-read against a single backend instance. Extracted
/// so the read_password retry path can call it with either backend.
async fn read_one_secret(
    backend: &Backend,
    attrs: &HashMap<&str, &str>,
) -> std::result::Result<Option<String>, oo7::Error> {
    let items = backend.search_items(attrs).await?;
    let Some(item) = items.into_iter().next() else { return Ok(None); };
    let secret = item.secret().await?;
    let utf8 = String::from_utf8(secret.to_vec()).map_err(|_| {
        // Wrap a "not valid UTF-8" failure as a synthetic file IO error
        // — it's the closest oo7 variant; the alternative is adding a
        // KeychainError variant just for this very-unlikely path.
        oo7::Error::File(oo7::file::Error::Io(
            std::io::Error::new(std::io::ErrorKind::InvalidData,
                                "stored password is not valid UTF-8")))
    })?;
    Ok(Some(utf8))
}

// ── Backend dispatch with deferred fallback ─────────────────────────────────
//
// `oo7::Keyring::new()` succeeds with a 0-byte portal key on Phosh —
// the WeakKey/PasswordTooShort signal only surfaces when we actually
// try to encrypt/decrypt via create_item. So the fallback decision
// can't happen at Keyring::new() time; it has to happen on the first
// keyring operation that actually touches the master key. We cache
// the chosen backend across calls so subsequent operations skip the
// portal probe.

use std::sync::atomic::{AtomicU8, Ordering};

const BACKEND_UNDECIDED: u8 = 0;
const BACKEND_PORTAL: u8 = 1;
const BACKEND_SELF_KEYED: u8 = 2;
static BACKEND_CHOICE: AtomicU8 = AtomicU8::new(BACKEND_UNDECIDED);

/// Open whichever backend the previous call decided on; if undecided,
/// open the portal/D-Bus path first and let the caller catch the
/// WeakKey error to trigger the fallback.
async fn open_chosen_backend() -> Result<Backend> {
    match BACKEND_CHOICE.load(Ordering::Acquire) {
        BACKEND_SELF_KEYED => open_self_keyed_backend().await,
        _ => match open_portal().await {
            Ok(kr) => Ok(kr),
            Err(e) => {
                // Even opening the portal/D-Bus failed (e.g. neither
                // is available). Fall back unconditionally and remember.
                crate::diag::log(&format!(
                    "keychain: portal/D-Bus open failed ({e}); using self-keyed file backend"));
                BACKEND_CHOICE.store(BACKEND_SELF_KEYED, Ordering::Release);
                open_self_keyed_backend().await
            }
        },
    }
}

/// Record the backend we just succeeded with so the next call skips
/// the probe. Self-keyed wins implicitly via `mark_backend_fallback`.
fn mark_backend_ok(backend: &Backend) {
    let v = match backend {
        Backend::Portal(_) => BACKEND_PORTAL,
        Backend::SelfKeyed(_) => BACKEND_SELF_KEYED,
    };
    BACKEND_CHOICE.store(v, Ordering::Release);
}

fn fall_back_log(err: &oo7::Error) {
    crate::diag::log(&format!(
        "keychain: portal returned an unusable master key ({err}); \
         falling back to self-keyed file backend"));
    BACKEND_CHOICE.store(BACKEND_SELF_KEYED, Ordering::Release);
}

/// Typed classifier — runs against the original `oo7::Error` BEFORE
/// any string conversion strips the variant info. Catches every
/// `WeakKey` sub-variant.
fn oo7_error_is_weak_key(err: &oo7::Error) -> bool {
    matches!(err, oo7::Error::File(oo7::file::Error::WeakKey(_)))
}

/// Unified backend wrapper. Both arms expose the same surface so the
/// `with_keyring` operation closure doesn't need to switch on type.
enum Backend {
    Portal(oo7::Keyring),
    SelfKeyed(oo7::file::Keyring),
}

impl Backend {
    async fn create_item(&self, label: &str, attrs: &HashMap<&str, &str>,
                         secret: &[u8], replace: bool)
        -> std::result::Result<(), oo7::Error>
    {
        match self {
            Backend::Portal(kr) => {
                kr.unlock().await?;
                kr.create_item(label, attrs, secret, replace).await?;
                Ok(())
            }
            Backend::SelfKeyed(kr) => {
                kr.create_item(label, attrs, secret, replace).await
                    .map(|_item| ())
                    .map_err(oo7::Error::File)
            }
        }
    }

    async fn search_items(&self, attrs: &HashMap<&str, &str>)
        -> std::result::Result<Vec<BackendItem>, oo7::Error>
    {
        match self {
            Backend::Portal(kr) => {
                kr.unlock().await?;
                Ok(kr.search_items(attrs).await?
                    .into_iter().map(BackendItem::Portal).collect())
            }
            Backend::SelfKeyed(kr) => {
                Ok(kr.search_items(attrs).await
                    .map_err(oo7::Error::File)?
                    .into_iter().map(BackendItem::SelfKeyed).collect())
            }
        }
    }

    async fn delete(&self, attrs: &HashMap<&str, &str>)
        -> std::result::Result<(), oo7::Error>
    {
        match self {
            Backend::Portal(kr) => {
                kr.unlock().await?;
                kr.delete(attrs).await?;
                Ok(())
            }
            Backend::SelfKeyed(kr) => {
                kr.delete(attrs).await.map_err(oo7::Error::File)
            }
        }
    }
}

enum BackendItem {
    Portal(oo7::Item),
    SelfKeyed(oo7::file::Item),
}

impl BackendItem {
    async fn secret(&self) -> std::result::Result<oo7::Secret, oo7::Error> {
        match self {
            BackendItem::Portal(item) => item.secret().await,
            BackendItem::SelfKeyed(item) => Ok(item.secret()),
        }
    }
}

async fn open_portal() -> Result<Backend> {
    Ok(Backend::Portal(oo7::Keyring::new().await?))
}

async fn open_self_keyed_backend() -> Result<Backend> {
    let key_path = self_keyed_master_path();
    let kr_path = self_keyed_keyring_path();
    let secret = load_or_create_master_key(&key_path)?;
    Ok(Backend::SelfKeyed(oo7::file::Keyring::load(&kr_path, secret).await?))
}

fn self_keyed_data_dir() -> PathBuf {
    glib::user_data_dir().join("meditate")
}
fn self_keyed_master_path() -> PathBuf {
    self_keyed_data_dir().join(SELF_KEYED_MASTER_FILENAME)
}
fn self_keyed_keyring_path() -> PathBuf {
    self_keyed_data_dir().join(SELF_KEYED_KEYRING_FILENAME)
}

/// Load the master-key file at `path`, or create it (32 random bytes,
/// mode 0600, parent directory created if missing) on first call.
/// Subsequent calls read the same bytes — that's how the file keyring
/// stays decryptable across app restarts.
fn load_or_create_master_key(path: &Path) -> Result<oo7::Secret> {
    if path.exists() {
        let bytes = std::fs::read(path)?;
        return Ok(oo7::Secret::from(bytes));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let secret = oo7::Secret::random()
        .map_err(|e| KeychainError::Backend(format!("getrandom: {e}")))?;
    write_secret_file(path, secret.as_bytes())?;
    Ok(secret)
}

/// Atomic-ish write of secret bytes with mode 0600. Fails if the file
/// already exists — the caller is supposed to have checked that, and a
/// race here would corrupt the existing key.
fn write_secret_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Pure-Rust tests cover the bits that don't need a running
    //! Secret Service or D-Bus session: error classification, master-
    //! key file management, schema/attribute constants. The actual
    //! store/read/delete round-trips need infrastructure and live in
    //! `bin/keychain_smoke`.

    use super::*;
    use oo7::file::{Error as FileError, WeakKeyError};

    // ── Schema / attributes ─────────────────────────────────────────────────

    #[test]
    fn schema_constant_namespaces_to_our_app_id() {
        // Renaming this orphans every keyring item the user has stored
        // — tripping a cheap test is much better than the user
        // discovering it the next time they open the app.
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
        // Renaming either of these orphans existing items; pin the
        // wire names in a test.
        assert_eq!(ATTR_URL, "url");
        assert_eq!(ATTR_USER, "username");
    }

    #[test]
    fn keychain_error_display_is_descriptive() {
        let err = KeychainError::Backend("D-Bus connection refused".to_string());
        assert_eq!(err.to_string(), "keychain: D-Bus connection refused");
    }

    // ── Backend-selection classifier ────────────────────────────────────────

    #[test]
    fn oo7_error_is_weak_key_matches_password_too_short() {
        // The exact case we hit on Phosh: portal call succeeds at the
        // protocol level but returns 0 bytes, oo7's file backend
        // refuses to use that as a master key, surfaces as
        // `WeakKey(PasswordTooShort)`. Our classifier MUST catch it.
        let e = oo7::Error::File(FileError::WeakKey(WeakKeyError::PasswordTooShort(0)));
        assert!(oo7_error_is_weak_key(&e),
            "PasswordTooShort must trigger the self-keyed fallback");
    }

    #[test]
    fn oo7_error_is_weak_key_matches_other_weak_key_variants() {
        // The fallback should fire for ALL WeakKey variants — a portal
        // that returns a too-short salt or too-low iteration count is
        // just as broken as one that returns an empty password. The
        // self-keyed backend is the right answer in every case.
        for v in [
            WeakKeyError::IterationCountTooLow(1),
            WeakKeyError::SaltTooShort(0),
            WeakKeyError::StrengthUnknown,
        ] {
            let e = oo7::Error::File(FileError::WeakKey(v));
            assert!(oo7_error_is_weak_key(&e),
                "every WeakKey variant should route to fallback: {e}");
        }
    }

    #[test]
    fn oo7_error_is_weak_key_does_not_match_legit_file_errors() {
        // `IncorrectSecret` means we have a master key that's wrong
        // for the existing keyring file (e.g. user deleted the master
        // file but kept the keyring). That's a real error — surfacing
        // it gives the user a chance to act. Don't silently fall back.
        let e = oo7::Error::File(FileError::IncorrectSecret);
        assert!(!oo7_error_is_weak_key(&e),
            "IncorrectSecret is a legitimate error, not a fallback signal");
    }

    #[test]
    fn oo7_error_is_weak_key_does_not_match_dbus_errors() {
        // The DBus-side errors aren't portal-related and shouldn't
        // fall back. (We construct a synthetic DBus error here — the
        // particular variant doesn't matter, just that it's not File.)
        // We can't easily construct a `dbus::Error` from outside the
        // crate, so we test the negative via a non-WeakKey File error
        // — the same `matches!` discrimination.
        let e = oo7::Error::File(FileError::NoData);
        assert!(!oo7_error_is_weak_key(&e),
            "non-WeakKey errors don't trigger the fallback");
    }

    // ── Master-key file management ──────────────────────────────────────────

    #[test]
    fn load_or_create_master_key_creates_file_with_random_bytes_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master");
        assert!(!path.exists());

        let secret = load_or_create_master_key(&path).unwrap();
        assert!(path.exists(), "master key file must be written");
        assert!(secret.as_bytes().len() >= 32,
            "secret must be substantial enough for HKDF — got {} bytes",
            secret.as_bytes().len());
        // First-call key must NOT be all zeros (would indicate a
        // failed RNG falling back to default-init).
        assert!(secret.as_bytes().iter().any(|&b| b != 0),
            "secret must come from getrandom, not default-init zeros");
    }

    #[test]
    fn load_or_create_master_key_returns_same_secret_on_subsequent_calls() {
        // The whole point of persisting the master is that we can
        // decrypt the keyring next time. A fresh random key on every
        // call would lock the user out of their own data.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master");
        let first = load_or_create_master_key(&path).unwrap();
        let second = load_or_create_master_key(&path).unwrap();
        assert_eq!(first.as_bytes(), second.as_bytes(),
            "subsequent calls must return the same persisted key");
    }

    #[test]
    fn load_or_create_master_key_creates_parent_directory_if_missing() {
        // First-run scenario on a fresh install: the meditate data
        // directory may not exist yet. The keychain init must create
        // it rather than failing.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("master");
        assert!(!path.parent().unwrap().exists());

        load_or_create_master_key(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    #[cfg(unix)]
    fn load_or_create_master_key_writes_with_mode_0600() {
        // The master key is the only thing protecting the encrypted
        // keyring's contents from inspection. Filesystem perms 0600
        // (owner-only read/write) are the actual security boundary on
        // Unix; an accidental 0644 would expose the key to anyone with
        // an account on the host. Pin it.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master");
        load_or_create_master_key(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600,
            "master key file must be owner-only, got {:o}", mode & 0o777);
    }

    #[test]
    fn load_or_create_master_key_two_calls_in_a_row_dont_corrupt() {
        // Edge case: if two threads / two processes hit the missing-
        // file path concurrently, only one should write. The other
        // should either see the file already exists and read it, OR
        // get a clean error. Our implementation uses `create_new` for
        // the write, so the second call to write_secret_file would
        // EEXIST — but the outer load_or_create checks `path.exists()`
        // first, so the realistic path is: A creates, B sees A's file
        // and reads it. Verify by running the call twice and
        // asserting the keys still match.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master");
        let a = load_or_create_master_key(&path).unwrap();
        let b = load_or_create_master_key(&path).unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes());
    }
}
