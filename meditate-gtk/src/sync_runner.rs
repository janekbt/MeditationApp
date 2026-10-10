//! One sync attempt: core's runner (`meditate_core::sync::runner`)
//! with this app's keyring and folders. Synchronous — meant to be
//! called from a worker thread (see `application::trigger_sync`).

use std::path::Path;

use meditate_core::sync::runner::{run_attempt, RunError};
use meditate_core::sync::SyncStats;

/// Run one sync attempt against the database at `db_path`; the
/// outcome lands in `sync_state` for the status indicator.
pub fn run_sync_attempt(db_path: &Path) -> Result<SyncStats, RunError> {
    run_attempt(db_path, local_sounds_dir(), local_guided_dir(), |url, username| {
        crate::keychain::read_password(url, username).map_err(|e| e.to_string())
    })
}

/// Canonical local directory for custom-imported bell-sound audio
/// files. Used by both the import flow (B.5) and the orchestrator's
/// sync push/pull paths.
pub fn local_sounds_dir() -> std::path::PathBuf {
    gtk::glib::user_data_dir().join("meditate").join("sounds")
}

/// Canonical local directory for imported guided-meditation OGG
/// files. Used by the guided import flow and the orchestrator's
/// sync push/pull paths.
pub fn local_guided_dir() -> std::path::PathBuf {
    gtk::glib::user_data_dir().join("meditate").join("guided")
}

// Connection test (TestConnectionResult + test_connection +
// test_connection_with) lives in `meditate_core::sync::credentials`.
pub use meditate_core::sync::credentials::{
    test_connection, test_connection_with, TestConnectionResult,
};

#[cfg(test)]
mod tests {
    use super::*;
    use meditate_core::sync::{FakeWebDav, WebDav};

    fn read(path: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
    }

    /// A file the server refused shows by name in the status, and
    /// Retry sends it again: automatic syncs skip it.
    #[test]
    fn refused_uploads_show_by_name_and_retry_sends_them_again() {
        let src = read("src/window/imp.rs");
        assert!(src.contains("SyncIndicatorState::NotUploaded(files) => {"));
        assert!(src.contains(
            "SyncIndicatorAction::RetrySync => {\n                        \
             app.with_db(|db| meditate_core::sync::settings::clear_refused_uploads(db.core()));\n                        \
             app.trigger_sync();"
        ));
    }

    /// A sync that never started for want of a password showed as
    /// healthy. Core sends the tap to the sync settings; it says so.
    #[test]
    fn a_missing_password_shows_in_the_status() {
        let src = read("src/window/imp.rs");
        assert!(src.contains("SyncIndicatorState::NeedsPassword => {"));
        assert!(src.contains("gettext(\"No password saved, click to enter it\")"));
    }

    /// Save kept an empty password field even with nothing stored for a
    /// changed URL, and Test sent the password over http. Both now run
    /// the same checks, so every error has its toast.
    #[test]
    fn save_and_test_check_the_password_and_https_alike() {
        let src = read("src/preferences.rs");
        assert!(!src.contains("unreachable!("));
        assert_eq!(src.matches("gettext(\"URL must start with https://\")").count(), 2);
        assert_eq!(src.matches("gettext(\"Enter a password\")").count(), 2);
        assert_eq!(src.matches("gettext(\"Keyring read failed\")").count(), 2);
    }

    // ── test_connection_with ─────────────────────────────────────────────────

    #[test]
    fn test_connection_with_succeeds_on_a_reachable_webdav() {
        // FakeWebDav's list_collection always returns Ok([]) for an
        // empty store — that's what we expect when the URL points at
        // a working but empty user root.
        let fs = FakeWebDav::new();
        assert_eq!(test_connection_with(&fs), TestConnectionResult::Ok);
    }

    /// Tiny scripted WebDav that returns a fixed error from every method.
    /// Easier than per-test inline impls and lets us exercise the error
    /// mapping branches one variant at a time.
    struct AlwaysErrs(meditate_core::WebDavError);
    impl WebDav for AlwaysErrs {
        fn list_collection(&self, _: &str)
            -> meditate_core::WebDavResult<Vec<String>>
        { Err(self.clone_err()) }
        fn get(&self, _: &str, _: u64)
            -> meditate_core::WebDavResult<Vec<u8>> { unreachable!() }
        fn put(&self, _: &str, _: &[u8])
            -> meditate_core::WebDavResult<()> { unreachable!() }
        fn mkcol(&self, _: &str)
            -> meditate_core::WebDavResult<()> { unreachable!() }
        fn delete(&self, _: &str)
            -> meditate_core::WebDavResult<()> { unreachable!() }
        fn move_to(&self, _: &str, _: &str)
            -> meditate_core::WebDavResult<()> { unreachable!() }
    }
    impl AlwaysErrs {
        fn clone_err(&self) -> meditate_core::WebDavError {
            use meditate_core::WebDavError as E;
            match &self.0 {
                E::NotFound => E::NotFound,
                E::Unauthorized => E::Unauthorized,
                E::Conflict => E::Conflict,
                E::Network(s) => E::Network(s.clone()),
                E::RateLimited { retry_after } =>
                    E::RateLimited { retry_after: *retry_after },
                E::Server { status, body } => E::Server {
                    status: *status, body: body.clone() },
                E::MalformedResponse { detail, body_excerpt } => E::MalformedResponse {
                    detail: detail.clone(),
                    body_excerpt: body_excerpt.clone(),
                },
                E::ResponseTooLarge { limit } => E::ResponseTooLarge { limit: *limit },
                E::Redirected { location } => E::Redirected { location: location.clone() },
            }
        }
    }

    #[test]
    fn test_connection_with_maps_401_to_unauthorized() {
        // Wrong app password is THE failure mode users will hit most.
        // The toast must read "Authentication failed" so they know to
        // re-check the password (not the URL, not the network).
        let w = AlwaysErrs(meditate_core::WebDavError::Unauthorized);
        assert_eq!(test_connection_with(&w), TestConnectionResult::Unauthorized);
    }

    #[test]
    fn test_connection_with_maps_dns_failure_to_network_error() {
        // The exact error pattern we hit on the Librem 5 with stale
        // resolver state — surface as Network, not as a generic Server
        // error, so the toast tells the user "couldn't reach" rather
        // than "server returned bad data".
        let w = AlwaysErrs(meditate_core::WebDavError::Network(
            "Dns Failed: ...".to_string()));
        assert_eq!(
            test_connection_with(&w),
            TestConnectionResult::Network("Dns Failed: ...".to_string()),
        );
    }

    #[test]
    fn test_connection_with_maps_404_to_not_webdav_root() {
        // Distinguishing 404 from generic-server-error matters because
        // the user-actionable advice is different: 404 means "fix the
        // URL"; 5xx means "wait / contact admin".
        let w = AlwaysErrs(meditate_core::WebDavError::NotFound);
        assert_eq!(
            test_connection_with(&w),
            TestConnectionResult::NotWebDavRoot,
        );
    }

    #[test]
    fn test_connection_with_routes_500_to_other() {
        // Server-side 500 isn't a config bug on our end, so the toast
        // should be diagnostic ("unexpected response") rather than
        // pointing fingers at the user's credentials or path.
        let w = AlwaysErrs(meditate_core::WebDavError::Server {
            status: 500, body: "internal".to_string() });
        match test_connection_with(&w) {
            TestConnectionResult::Other(s) => {
                assert!(s.contains("500"),
                    "Other variant must include the status code, got: {s}");
            }
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn test_connection_result_display_text_is_actionable() {
        // Display IS the toast text — kept short so it fits on the
        // Librem 5 viewport. Pin the wording so a future copy edit
        // doesn't accidentally let it grow back into the cut-off zone.
        assert_eq!(TestConnectionResult::Ok.to_string(), "Connection OK");
        assert_eq!(
            TestConnectionResult::Unauthorized.to_string(),
            "Authentication failed",
        );
        assert_eq!(
            TestConnectionResult::NotWebDavRoot.to_string(),
            "Not a WebDAV folder",
        );
        // Inner string is dropped from Display (it goes to detail()
        // for the diag log), so the toast doesn't balloon when the
        // underlying error is verbose.
        assert_eq!(
            TestConnectionResult::Network("Dns Failed: long verbose msg".into())
                .to_string(),
            "Network error",
        );
        assert_eq!(
            TestConnectionResult::Other("HTTP 503: a long body".into()).to_string(),
            "Server error",
        );
    }

    #[test]
    fn test_connection_result_detail_includes_inner_strings() {
        // detail() goes to the diagnostics log — it MUST include the
        // underlying error for Network/Other variants so a user who
        // sends the log can be helped without guessing.
        assert!(
            TestConnectionResult::Network("Dns Failed: x".into())
                .detail().contains("Dns Failed: x"),
            "Network detail must contain the inner error",
        );
        assert!(
            TestConnectionResult::Other("HTTP 503".into())
                .detail().contains("HTTP 503"),
            "Other detail must contain the inner error",
        );
        // The Ok / Unauthorized / NotWebDavRoot variants don't carry
        // payload — detail() just emits a fuller human-readable form.
        assert!(TestConnectionResult::Ok.detail().contains("Connection OK"));
        assert!(TestConnectionResult::Unauthorized.detail().contains("401"));
        assert!(TestConnectionResult::NotWebDavRoot.detail().contains("404"));
    }
}
