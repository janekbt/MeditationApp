# Tests that catch the bug

## Why
Some tests stay green when the code they guard breaks (batch R: #91,
#138, #150). The "push my local data" recovery, the pull size cap and
the folder handling against Nextcloud are tested against a fake server
that is kinder than a real one. Android's Log grouping and the Box
Breath dot never run in a test. A renamed Kotlin method silently stops
bells, vibration, the keychain or the service.

## What Changes
- #91: the recovery test starts from synced events and a fresh peer must receive the old session; the size-cap tests put the oversized file straight on the server (10 MB + 1 byte, and over 11 MB); a strict test server answers like Nextcloud (MKCOL on an existing folder refused, PUT into a missing folder refused, listing a missing folder is "not found"), with a two-round sync carrying custom sounds and guided files.
- #138: `group_log_sessions`, `truncate_note_for_card`, `rounded_perimeter_point` and the two Log data structs move from `ui.rs` to `app.rs` unchanged, the clock format passed in; real-input tests replace the two source-text assertions on `group_log_sessions`.
- #150: one table test pins every Rust `call_static_method` into our Kotlin classes: method name and JNI signature against the `@JvmStatic fun` line in the Kotlin file (types translated), the class constant against the Kotlin package and object, and no call missing from the table.
- No behaviour change in either app.
- Left out: the "…" note shortening stays as it is (batch T, #159 changes it); Kotlin methods Kotlin itself calls are not part of the JNI table.

## Decisions
- Scope as said back, approved 2026-10-22 ("Go").
- The strict server is a test-only wrapper in the orchestrator tests, not a mode of `FakeWebDav` (my call; the issue asks for a wrapper).
- #150 gets no spec delta: it guards a mechanism, nothing a user sees changes (my call).
- The note shortening gets a test but no scenario, so the spec doesn't fix "…" in place before #159 (my call).

## Capabilities
### Modified Capabilities
- `sync`: recovery, size cap and folder handling get scenarios.
- `log`: Android's day grouping gets scenarios.
- `box-breath`: Android's dot path gets a scenario.

## Impact
- Core: `sync/settings.rs`, `sync/orchestrator.rs` tests only.
- GTK: none.
- Android: `src/ui.rs` (helpers removed, callers pass the clock format), `src/app.rs` (helpers and tests), one new JNI table test in `src/app.rs`.
- Translations: none.
- Data: none.
- Android build environment: untouched.
- Docs: OPEN-ISSUES.md (batch R ticked).
