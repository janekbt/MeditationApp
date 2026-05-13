//! Test-only helper macros shared across the crate's `#[cfg(test)]`
//! modules. `std::assert_matches!` is stable since 1.82 but our
//! MSRV is 1.75, so we ship our own.

/// Assert that `$expr` matches `$pat`. Panics with the actual value
/// (Debug-formatted) when it doesn't. Two shapes:
///
/// ```ignore
/// // Pure pattern check.
/// assert_matches!(err, SyncError::WebDav(WebDavError::Unauthorized));
///
/// // Pattern + body to run when matched (e.g., for further
/// // assertions on bound captures).
/// assert_matches!(
///     action,
///     PreviewAction::StopAndStart { id, .. } => assert_eq!(id, "pattern-a"),
/// );
/// ```
///
/// Mirrors `std::assert_matches::assert_matches!`; when MSRV moves
/// past 1.82 this module can be deleted and call sites switched to
/// the std version verbatim.
macro_rules! assert_matches {
    ($expr:expr, $pat:pat $(if $guard:expr)? $(,)?) => {
        match $expr {
            $pat $(if $guard)? => {}
            ref other => panic!(
                "assert_matches!: expected {}, got {:?}",
                stringify!($pat $(if $guard)?),
                other,
            ),
        }
    };
    ($expr:expr, $pat:pat $(if $guard:expr)? => $body:expr $(,)?) => {
        match $expr {
            $pat $(if $guard)? => { $body }
            ref other => panic!(
                "assert_matches!: expected {}, got {:?}",
                stringify!($pat $(if $guard)?),
                other,
            ),
        }
    };
}

pub(crate) use assert_matches;
