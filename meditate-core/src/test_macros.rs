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

/// Assert that two `f64`s are bit-identical. For tests asserting on a
/// function-under-test's exact return value where the math IS exact
/// (integer divisions with non-zero divisors, capped ratios, sentinel
/// 0.0 / 1.0), this is the clippy-clean alternative to
/// `assert_eq!(actual, expected)` — the bit-pattern comparison
/// sidesteps `clippy::float_cmp` without an epsilon dance.
///
/// Failure message is human-readable (the floats themselves, not the
/// underlying u64 bits): `assert_f64_eq!: expected 0.0, got 0.0000…`.
macro_rules! assert_f64_eq {
    ($actual:expr, $expected:expr $(,)?) => {{
        let actual: f64 = $actual;
        let expected: f64 = $expected;
        assert_eq!(
            actual.to_bits(),
            expected.to_bits(),
            "assert_f64_eq!: expected {expected}, got {actual}",
        );
    }};
    ($actual:expr, $expected:expr, $($msg:tt)+) => {{
        let actual: f64 = $actual;
        let expected: f64 = $expected;
        assert_eq!(
            actual.to_bits(),
            expected.to_bits(),
            "{}: expected {expected}, got {actual}",
            format_args!($($msg)+),
        );
    }};
}

pub(crate) use assert_f64_eq;
