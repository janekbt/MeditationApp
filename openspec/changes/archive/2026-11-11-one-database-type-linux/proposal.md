# One database type on Linux

## Why
The Linux app wraps core's database in a second `Database` type with
its own `DbError` that copies core's variants one to one, about 60
methods that only pass calls on and map the error, and a `clash_text`
that downcasts both error types. 88 calls already bypass the wrapper
through `db.core()`, so every caller uses two ways for the same thing.
Android uses core directly. REFACTORING #328, the last refactor of
package P3. Nothing a user or another device sees changes.

## What Changes
- The shell `DbError`, its `Display`, `From`, `map_core_err` and the
  `Result` alias go; the GTK app uses `meditate_core::db::{DbError, Result}`.
- `clash_text` reads only core's error: a free
  `duplicate_toast_text(&meditate_core::db::DbError)` in `db/mod.rs`
  keeps the gettext "«{name}» is already taken".
- The wrapper struct goes: `crate::db::Database` is core's
  `meditate_core::Database` (re-export). `db.core()` disappears from
  its 88 call sites; wrapper calls with the same name on core keep
  working unchanged; callers of wrappers over core's free `*_from_db`
  functions call those functions (as Android does).
- Kept, because only Linux needs them: a free `crate::db::open(path)` (creates the
  folder, opens, seeds the bundled bells from the GResource table, then
  `seed_all_non_audio`), the bundled bell table, and the unix-time
  `Session`/`SessionData` shim as free functions `create_session`,
  `save_ended_session`, `list_sessions`, `update_session`.
- Also from #328: `list_presets`, `active_months`, `active_days_in_month`
  (no caller) go with the wrappers; `let _ = BellSoundCategory::General;`
  goes; `test_db_in_memory` uses `seed_all_non_audio()`; stale module
  docs (rusqlite `Result`, "delete the old DB") are rewritten; the seed
  doc moves from `bundled_bell_resource` to the table.
- The shell `DbError`'s `Display` and `std::error::Error` impls move
  to core's `DbError` with the same texts, so every `{e}` and
  `to_string()` on a database error logs what it logs today.
- `application.rs` maps an open failure with core's existing
  `db_open_failure_key`; `data_io.rs`'s `From<DbError>` takes core's
  error, with the same text.
- Left out: unifying core's free `*_from_db` functions into methods
  (core API change for both apps, not needed to remove the shell
  layer).

## Decisions
- Approved say-back 2026-11-11 ("go").
- Re-export core's type instead of keeping a slimmer newtype or a
  `Deref` (my call): one type, no second way to reach core.
- Shell-only helpers are free functions in `crate::db`, not methods
  (my call: Rust cannot add inherent methods to a foreign type, and an
  extension trait would need an import at every caller).
- Core's `Database::open` stays as it is; the Linux opener is the free
  `crate::db::open`, and a test keeps every Linux caller on it (my
  call, from the builder questions: a re-exported type would otherwise
  silently reach core's `open` and skip the bundled bells).

## Capabilities
None (pure refactor, `skip_specs: true`).

## Impact
- Core: `db/error.rs` gets `Display` and `std::error::Error` for `DbError` (moved from the shell, same texts).
- GTK: `db/mod.rs`, and every file that calls the database
  (`application.rs`, `data_io.rs`, `labels.rs`, `vibrations.rs`,
  `vibration_editor.rs`, `guided.rs`, `presets.rs`, `bells.rs`,
  `sound.rs`, `sounds.rs`, `local_folder.rs`, `preferences.rs`,
  `recovery_dialog.rs`, `sync_runner.rs`, `window/imp.rs`,
  `log/imp.rs`, `stats/imp.rs`, `timer/imp.rs`).
- Android: none.
- Translations: none (the "«{name}» is already taken" msgid stays).
- Data: none.
- Android build environment: untouched.
- Docs: REFACTORING.md (#328 out, P3 row), ARCHITECTURE.md if it
  describes the shell wrapper (grep).
