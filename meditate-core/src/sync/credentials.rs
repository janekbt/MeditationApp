//! Sync-credentials validation + connection test.
//!
//! The Preferences → Data page's Save and Test Connection buttons run
//! near-identical validation chains over `(url, username, typed
//! password, stored password)`. The decisions — trim, reject empty,
//! fall back to keychain, enforce HTTPS — are portable; the keychain
//! access and the HTTP transport are shell-specific. These helpers own
//! the decision shape so every shell agrees on the rules.
//!
//! Pairs with `sync::settings` which owns the persisted account (URL,
//! username, sync-status timestamps). This module is concerned only
//! with what the user is currently typing, not what's already on disk.

use crate::sync::{WebDav, WebDavError};
use std::fmt;

// ── Connection test ──────────────────────────────────────────────────
//
// User-facing "Test connection" button in the sync settings dialog.
// Validates a (URL, username, password) tuple by issuing a single
// PROPFIND against the user's WebDAV root — cheap, doesn't touch the
// local DB or keychain, doesn't write anything to the remote. Maps
// the typed `WebDavError` variants to user-readable outcomes.

/// Outcome of a connection test. Display impl is the toast text.
#[derive(Debug, PartialEq, Eq)]
pub enum TestConnectionResult {
    /// PROPFIND returned 207 (Multi-Status) — auth + URL are good.
    Ok,
    /// 401 — credentials wrong (username, app-password, or both).
    Unauthorized,
    /// DNS / connection refused / timeout — couldn't reach the host.
    /// The string is the underlying error for diagnostics.
    Network(String),
    /// 404 — the URL points somewhere that exists but isn't a WebDAV
    /// folder. Almost always a typo in the path component.
    NotWebDavRoot,
    /// Anything else: 5xx, malformed XML, etc.
    Other(String),
}

impl fmt::Display for TestConnectionResult {
    /// Toast text — kept terse so it fits on narrow viewports
    /// (Librem 5 truncates around 30 chars). Longer diagnostic
    /// strings live in `detail()` and go to the diagnostics log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "Connection OK"),
            Self::Unauthorized => write!(f, "Authentication failed"),
            Self::Network(_) => write!(f, "Network error"),
            Self::NotWebDavRoot => write!(f, "Not a WebDAV folder"),
            Self::Other(_) => write!(f, "Server error"),
        }
    }
}

impl TestConnectionResult {
    /// Detailed text for the diagnostics log — includes the
    /// underlying error string for Network/Other so post-hoc
    /// debugging has the full picture even though the toast is short.
    pub fn detail(&self) -> String {
        match self {
            Self::Ok => "Connection OK".to_string(),
            Self::Unauthorized => "Authentication failed (HTTP 401)".to_string(),
            Self::Network(s) => format!("Network error: {s}"),
            Self::NotWebDavRoot => "URL is not a WebDAV folder (HTTP 404)".to_string(),
            Self::Other(s) => format!("Server error: {s}"),
        }
    }
}

/// Transport-agnostic connection test. Lifts the `WebDav` trait so
/// unit tests can pass a fake impl that produces specific error
/// variants. The shell composes this with `HttpWebDav` via
/// `test_connection` for the real "Test connection" button.
pub fn test_connection_with<W: WebDav>(webdav: &W) -> TestConnectionResult {
    match webdav.list_collection("/") {
        Ok(_) => TestConnectionResult::Ok,
        Err(WebDavError::Unauthorized) => TestConnectionResult::Unauthorized,
        Err(WebDavError::Network(s)) => TestConnectionResult::Network(s),
        Err(WebDavError::NotFound) => TestConnectionResult::NotWebDavRoot,
        Err(e) => TestConnectionResult::Other(e.to_string()),
    }
}

/// Real connection test against `HttpWebDav`. Synchronous — caller
/// runs from a worker thread so the UI doesn't freeze on slow
/// networks. Doesn't touch local state.
pub fn test_connection(url: &str, username: &str, password: &str) -> TestConnectionResult {
    let webdav = crate::sync::HttpWebDav::new(url, username, password);
    test_connection_with(&webdav)
}

// ── Save/Test prep ──────────────────────────────────────────────────

/// Failure modes the save-button validation chain surfaces. Shell
/// maps each variant to its gettext toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSyncError {
    EmptyUrl,
    EmptyUsername,
    /// URL scheme isn't `https://` (or a missing scheme entirely).
    /// HTTP would send the Basic-auth password in cleartext on every
    /// request — the auth header is just `base64(user:pw)` over the
    /// wire — so the sync layer refuses to attempt it. Shell maps
    /// this to a user-facing "URL must start with https://" toast.
    InsecureUrl,
}

/// What the shell should do with the password row's contents after
/// the user taps Save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordAction {
    /// User left the password row empty — keep the existing
    /// keychain entry untouched. Avoids clobbering on a "fix the
    /// URL typo, leave password alone" edit.
    Keep,
    /// User typed a non-empty password — write it to the keychain.
    Store(String),
}

/// Resolved plan from a successful Save validation. Shell calls
/// `keychain::store_password` per `password` then
/// `set_nextcloud_account(url, username)` then `trigger_sync`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSyncPlan {
    pub url: String,
    pub username: String,
    pub password: PasswordAction,
}

/// Validate the user's Save-button input. Trims url + username,
/// rejects empty, decides whether the typed password should be
/// stored or skipped. The actual keychain write + DB update +
/// `trigger_sync` ordering stays in the shell (it owns the
/// keychain transport + the threading model).
pub fn prepare_save(
    url: &str,
    username: &str,
    typed_password: &str,
) -> std::result::Result<SaveSyncPlan, SaveSyncError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(SaveSyncError::EmptyUrl);
    }
    // URL schemes are case-insensitive per RFC 3986; lowercase
    // before comparing so "HTTPS://…" and "Https://…" pass.
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err(SaveSyncError::InsecureUrl);
    }
    let username = username.trim();
    if username.is_empty() {
        return Err(SaveSyncError::EmptyUsername);
    }
    let password = if typed_password.is_empty() {
        PasswordAction::Keep
    } else {
        PasswordAction::Store(typed_password.to_string())
    };
    Ok(SaveSyncPlan {
        url: url.to_string(),
        username: username.to_string(),
        password,
    })
}

/// Failure modes the Test-Connection button's prerequisite check
/// surfaces. Shell maps each to a gettext toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestPrereq {
    EmptyUrl,
    EmptyUsername,
    /// User left the password row empty AND no keychain entry
    /// exists — they need to type one in.
    NoPassword,
    /// Keychain access failed (D-Bus error, locked keyring, etc.).
    /// Shell logs the underlying error to diag; the toast just
    /// signals the user to try again.
    KeyringFailed,
}

/// Outcome of the shell's keychain lookup for the stored password.
/// Carries the three states the prep function dispatches on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredPassword {
    /// Keychain returned a password for the configured account.
    Found(String),
    /// Keychain succeeded but had no entry.
    Missing,
    /// Keychain access errored out.
    Failed,
}

/// Validated credentials ready for the `test_connection` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub url: String,
    pub username: String,
    pub password: String,
}

/// Validate the user's Test-Connection input. Trims url + username,
/// rejects empty, falls back to the keychain when typed password is
/// empty. The shell supplies the keychain read via the closure so
/// the helper stays portable.
pub fn prepare_test(
    url: &str,
    username: &str,
    typed_password: &str,
    stored_password: impl FnOnce() -> StoredPassword,
) -> std::result::Result<Credentials, TestPrereq> {
    let url = url.trim();
    if url.is_empty() {
        return Err(TestPrereq::EmptyUrl);
    }
    let username = username.trim();
    if username.is_empty() {
        return Err(TestPrereq::EmptyUsername);
    }
    let password = if typed_password.is_empty() {
        match stored_password() {
            StoredPassword::Found(p) => p,
            StoredPassword::Missing => return Err(TestPrereq::NoPassword),
            StoredPassword::Failed => return Err(TestPrereq::KeyringFailed),
        }
    } else {
        typed_password.to_string()
    };
    Ok(Credentials {
        url: url.to_string(),
        username: username.to_string(),
        password,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{FakeWebDav, WebDavResult};
    use crate::test_macros::assert_matches;

    // ── test_connection_with ─────────────────────────────────────────────

    /// Tiny scripted WebDav that returns a fixed error from every
    /// method. Lets us exercise the error-mapping branches one
    /// variant at a time without touching the network.
    struct AlwaysErrs(WebDavError);

    impl AlwaysErrs {
        fn clone_err(&self) -> WebDavError {
            match &self.0 {
                WebDavError::NotFound => WebDavError::NotFound,
                WebDavError::Unauthorized => WebDavError::Unauthorized,
                WebDavError::Conflict => WebDavError::Conflict,
                WebDavError::Network(s) => WebDavError::Network(s.clone()),
                WebDavError::RateLimited { retry_after } => {
                    WebDavError::RateLimited { retry_after: *retry_after }
                }
                WebDavError::Server { status, body } => WebDavError::Server {
                    status: *status,
                    body: body.clone(),
                },
                WebDavError::MalformedResponse { detail, body_excerpt } => {
                    WebDavError::MalformedResponse {
                        detail: detail.clone(),
                        body_excerpt: body_excerpt.clone(),
                    }
                }
            }
        }
    }

    impl WebDav for AlwaysErrs {
        fn list_collection(&self, _: &str) -> WebDavResult<Vec<String>> {
            Err(self.clone_err())
        }
        fn get(&self, _: &str) -> WebDavResult<Vec<u8>> { unreachable!() }
        fn put(&self, _: &str, _: &[u8]) -> WebDavResult<()> { unreachable!() }
        fn mkcol(&self, _: &str) -> WebDavResult<()> { unreachable!() }
        fn delete(&self, _: &str) -> WebDavResult<()> { unreachable!() }
        fn move_to(&self, _: &str, _: &str) -> WebDavResult<()> { unreachable!() }
    }

    #[test]
    fn test_connection_with_ok_when_propfind_succeeds() {
        let fake = FakeWebDav::new();
        assert_eq!(test_connection_with(&fake), TestConnectionResult::Ok);
    }

    #[test]
    fn test_connection_with_unauthorized_when_propfind_401s() {
        let w = AlwaysErrs(WebDavError::Unauthorized);
        assert_eq!(test_connection_with(&w), TestConnectionResult::Unauthorized);
    }

    #[test]
    fn test_connection_with_network_error_carries_underlying_string() {
        let w = AlwaysErrs(WebDavError::Network("Dns Failed: ...".into()));
        assert_eq!(
            test_connection_with(&w),
            TestConnectionResult::Network("Dns Failed: ...".to_string()),
        );
    }

    #[test]
    fn test_connection_with_not_webdav_root_when_404() {
        let w = AlwaysErrs(WebDavError::NotFound);
        assert_eq!(
            test_connection_with(&w),
            TestConnectionResult::NotWebDavRoot,
        );
    }

    #[test]
    fn test_connection_with_other_for_server_errors() {
        let w = AlwaysErrs(WebDavError::Server {
            status: 500,
            body: "internal".into(),
        });
        assert_matches!(test_connection_with(&w), TestConnectionResult::Other(_));
    }

    // ── prepare_save ────────────────────────────────────────────────

    #[test]
    fn prepare_save_rejects_empty_url() {
        assert_eq!(prepare_save("", "user", "pw"), Err(SaveSyncError::EmptyUrl));
        assert_eq!(prepare_save("   ", "user", "pw"), Err(SaveSyncError::EmptyUrl));
    }

    #[test]
    fn prepare_save_rejects_empty_username() {
        assert_eq!(
            prepare_save("https://nx.example", "", "pw"),
            Err(SaveSyncError::EmptyUsername),
        );
        assert_eq!(
            prepare_save("https://nx.example", "  ", "pw"),
            Err(SaveSyncError::EmptyUsername),
        );
    }

    #[test]
    fn prepare_save_rejects_http_url() {
        // Basic-auth over HTTP sends `base64(user:pw)` in cleartext
        // on every request — the sync layer refuses to attempt it.
        assert_eq!(
            prepare_save("http://nx.example", "user", "pw"),
            Err(SaveSyncError::InsecureUrl),
        );
    }

    #[test]
    fn prepare_save_rejects_scheme_less_url() {
        // A typo without scheme would otherwise fall through to ureq
        // which infers http — also cleartext.
        assert_eq!(
            prepare_save("nx.example", "user", "pw"),
            Err(SaveSyncError::InsecureUrl),
        );
    }

    #[test]
    fn prepare_save_rejects_non_http_schemes() {
        // Belt-and-braces: only https is accepted. ftp, file,
        // gemini, anything else gets the same insecure-url toast.
        assert_eq!(
            prepare_save("ftp://nx.example", "user", "pw"),
            Err(SaveSyncError::InsecureUrl),
        );
    }

    #[test]
    fn prepare_save_accepts_https_url_case_insensitively() {
        // RFC 3986: URL schemes are case-insensitive.
        for scheme in ["https://", "HTTPS://", "Https://", "HttPs://"] {
            let url = format!("{scheme}nx.example");
            let plan = prepare_save(&url, "user", "pw")
                .unwrap_or_else(|e| panic!("{url} should be accepted, got {e:?}"));
            assert_eq!(plan.username, "user");
        }
    }

    #[test]
    fn prepare_save_keep_when_password_empty() {
        let plan = prepare_save("https://nx.example", "user", "").unwrap();
        assert_eq!(plan.password, PasswordAction::Keep);
        assert_eq!(plan.url, "https://nx.example");
        assert_eq!(plan.username, "user");
    }

    #[test]
    fn prepare_save_store_when_password_present() {
        let plan = prepare_save("https://nx.example", "user", "secret").unwrap();
        assert_eq!(plan.password, PasswordAction::Store("secret".into()));
    }

    #[test]
    fn prepare_save_trims_url_and_username() {
        let plan = prepare_save("  https://nx.example  ", "  user  ", "x").unwrap();
        assert_eq!(plan.url, "https://nx.example");
        assert_eq!(plan.username, "user");
    }

    // ── prepare_test ────────────────────────────────────────────────

    #[test]
    fn prepare_test_rejects_empty_url() {
        assert_eq!(
            prepare_test("", "user", "pw", || StoredPassword::Missing),
            Err(TestPrereq::EmptyUrl),
        );
    }

    #[test]
    fn prepare_test_rejects_empty_username() {
        assert_eq!(
            prepare_test("https://nx", "", "pw", || StoredPassword::Missing),
            Err(TestPrereq::EmptyUsername),
        );
    }

    #[test]
    fn prepare_test_uses_typed_password_directly() {
        let creds = prepare_test(
            "https://nx", "user", "typed",
            || panic!("should not be called when typed_pw is non-empty"),
        ).unwrap();
        assert_eq!(creds.password, "typed");
    }

    #[test]
    fn prepare_test_falls_back_to_stored_password_when_typed_empty() {
        let creds = prepare_test(
            "https://nx", "user", "",
            || StoredPassword::Found("from-keyring".into()),
        ).unwrap();
        assert_eq!(creds.password, "from-keyring");
    }

    #[test]
    fn prepare_test_returns_no_password_when_typed_empty_and_keyring_empty() {
        assert_eq!(
            prepare_test("https://nx", "user", "", || StoredPassword::Missing),
            Err(TestPrereq::NoPassword),
        );
    }

    #[test]
    fn prepare_test_returns_keyring_failed_on_keychain_error() {
        assert_eq!(
            prepare_test("https://nx", "user", "", || StoredPassword::Failed),
            Err(TestPrereq::KeyringFailed),
        );
    }
}
