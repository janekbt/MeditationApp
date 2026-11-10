# One path per table

## Why
Each core table reads, inserts, edits and deletes its rows through
several hand-written copies of the same code. A new column means up to
six edits, and the copies have already drifted (the session bulk insert
writes a raw label id the single insert would clear; the "nothing
changed" guard exists in five shapes). This is the second half of
REFACTORING package P1: #235, #237, #215, #216, #217, #231, plus the
notes that change writes-keep-peer-edits added to them. Nothing a user
or another device sees changes.

## What Changes
- One row reader per table (`COLUMNS` const plus `from_row`, as
  `local_guided_files.rs` does) for sessions, events, bell sounds,
  vibration patterns, guided files, Box Breath phases and interval
  bells; every list and keyed lookup uses it. Interval bells and bell
  sounds get a keyed lookup instead of loading the whole list (#235,
  #216, `session_by_id`).
- One edit shape per table: `edit_*_in(tx, uuid, FnOnce(&mut Row))`
  loads the row, applies the edit, writes nothing when it is equal,
  checks a changed name, then writes. Callers that already hold the
  row (`rename_vibration_pattern`, `set_interval_bell_enabled`) use it
  instead of reading twice (#235 note).
- One delete helper for the synced tables (existence, DELETE,
  tombstone) and one `delete_row` for the no-winner branch of every
  rebuild (#237). Sessions: `delete_session` finds the uuid and calls
  `delete_session_by_uuid`; `delete_all_sessions` uses the same
  tombstone helper (#217).
- One insert path for sessions: `insert_session_tx_less` goes through
  `insert_sessions_in`, which saves a deleted label as none like the
  single path does. `bulk_insert_sessions` and its unused GTK wrapper
  are deleted; their tests move onto `insert_imported` or go when they
  duplicate one (#215).
- Repeated helpers merged (#231): `order_key::fresh_keys(n)`; table
  parameterised known-remote record/list/wipe; one `seed_once`; one
  label uuid-by-id and id-by-uuid lookup; `write_kv` in `settings.rs`;
  `existing_rowid_by_uuid` where hand-written; `mint_uuid()` where
  uuids are minted by hand; `.optional()` in `mod.rs`; the Box Breath
  phase names in `error.rs` from `BoxBreathPhaseId`; the inline event
  swap copies in tests use `test_helpers::{sent, push, exchange}`.
- Left out: batch K3 (behaviour fixes, its own change next, maintainer's
  choice); sessions keep their own SQL writes instead of the rebuild
  per row (import speed, decided in writes-keep-peer-edits); #232's
  small items stay in their packages.

## Decisions
- K3 is its own change after this one (maintainer, 2026-11-10).
- Public function names the apps call stay as they are; only their
  bodies share the new readers, so GTK and Android change only where a
  function is deleted (`bulk_insert_sessions`). (my call)
- No spec deltas: a pure refactor (`skip_specs: true`). Existing tests
  pin the behaviour; where a touched function has none, a pinning test
  comes first. (my call)

## Capabilities
### New Capabilities
- none
### Modified Capabilities
- none (no behaviour changes)

## Impact
- Core: `db/` sessions, events, bell_sounds, vibration_patterns,
  guided_files, box_breath_phases, interval_bells, presets, labels,
  starred_order, order_key, known_remote, seeds, settings, device,
  error, mod, session_in_progress, local_changes; `preset_config.rs`
  tests.
- GTK: `src/db/mod.rs` (the unused bulk-insert wrapper goes).
- Android: none expected (checked by building).
- Translations: none.
- Data: none (no schema, payload or event change).
- Android build environment: untouched.
- Docs: REFACTORING.md (done items out, P1 ticked), ARCHITECTURE.md
  only if it names a removed function.
