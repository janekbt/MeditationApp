# Session recovery — design plan

Implementation plan for the "battery-death / OOM mid-session = whole
session lost" item that was parked from the 2026-05-12 Tier-0 pass.
Drafted 2026-05-13.

## Problem

Sessions are only persisted when `on_complete` fires
(`src/timer/imp.rs:2380`). If the OS kills the process partway
through — kernel OOM, battery dies, Rust panic, Android lifecycle
swap — the meditation that already happened is lost: no row in
`sessions`, no contribution to streak / daily total / per-label
aggregate / log card. Linux probability is low; Android (future)
probability is meaningful because the system routinely kills
backgrounded processes.

The work the user already did should count.

## Design

### Persist current-session state in a non-event-sourced table

Add a single-row `session_in_progress` table that lives **outside**
the event log. The shell writes to it as the session progresses;
core finalizes it (or auto-finalizes on next launch) by emitting
one `session_insert` event with the captured duration.

Key properties:

- Mutating this table does **not** call `emit_event`. Sync sees
  nothing while a session is in progress. One event per completed
  session, same footprint as today.
- The row is device-local. We do not sync "I am currently
  meditating" across the user's devices.
- The auto-finalize path on next launch produces exactly the same
  event a normal completion would, so there is no parallel code
  path in the sync / replay machinery.

### Recovery UX: auto-finalize + Undo toast

On `application::startup`:

1. Read `session_in_progress`.
2. If `Some`, call `finalize_session_in_progress` (writes the
   `session_insert` event using the captured `accumulated_secs`,
   clears the in-progress row, all in one transaction).
3. `app.invalidate(InvalidateScope::ALL)` so the new session shows
   up in stats.
4. Surface an `adw::Toast` on the active window: *"Saved your
   previous session of 47 min. Undo?"* with a 4–6s timeout and an
   Undo button that calls `delete_session` on the freshly-inserted
   row.

No blocking dialog. Common case is zero-click — the user simply
sees the session in their log. "Discard" becomes "tap Undo."

### Shape

```rust
pub struct SessionInProgress {
    pub start_iso: String,           // when the session began
    pub accumulated_secs: u32,       // elapsed time as of the last tick
    pub mode: SessionMode,           // Timer / BoxBreath / Guided
    pub mode_payload: String,        // JSON, opaque to core
    pub label_id: Option<i64>,       // local rowid (device-local table)
    pub guided_file_uuid: Option<String>,
}
```

Schema:

```sql
CREATE TABLE IF NOT EXISTS session_in_progress (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    start_iso        TEXT    NOT NULL,
    accumulated_secs INTEGER NOT NULL,
    mode             TEXT    NOT NULL CHECK (mode IN ('timer', 'box_breath', 'guided')),
    mode_payload     TEXT    NOT NULL,
    label_id         INTEGER         REFERENCES labels(id) ON DELETE SET NULL,
    guided_file_uuid TEXT
);
```

Storing `label_id` (local rowid) rather than `label_uuid` keeps
the in-progress row simple — `insert_session` already resolves
`label_id → label_uuid` at event-emission time, so finalize gets
the cross-device translation for free.

No `phase` column. `accumulated_secs` freezes on pause and grows
on resume — that's the only information the recovery flow needs.

### Decisions locked in

- **No Resume option in the recovery UX.** If the app crashed
  mid-meditation the meditative state is already broken; the
  value is preserving the time, not the place. Resume can be a
  v2 if it turns out to matter.
- **No snapshot blob in `settings`** — the dedicated table is
  cleaner and avoids any ambiguity about whether a setting key
  is "in-progress data" or a real preference.
- **No phase tracking.** Paused vs Running collapses to "is the
  accumulated_secs counter currently advancing." The shell knows
  which it is; the row doesn't need to.

## Implementation order

Strict TDD, one logical change per commit. All laptop-testable —
no haptic / vibration / playback work, so commit on green tests
without waiting for Librem verification (verify after.)

### Core — `meditate-core` (estimate: 2–3 hours, 4 commits)

1. **Add `session_in_progress` table to `db/schema.rs`** + a
   `CACHE_SCHEMA_VERSION` bump if needed. Tests:
   `Database::open_in_memory` creates the table; the CHECK
   constraint rejects `id != 1`.
2. **Create `db/session_in_progress.rs`** with `SessionInProgress`
   struct + `set_session_in_progress`, `get_session_in_progress`,
   `clear_session_in_progress`. **Critical invariant test:**
   mutating the in-progress table emits zero events
   (`pending_events()` stays empty across set/get/clear cycles).
3. **Add `finalize_session_in_progress`** — single transaction
   that reads the in-progress row, calls the existing
   `insert_session` path (which emits the one `session_insert`
   event), and clears the in-progress row. Test the sync event
   appears, the new session row matches the captured snapshot,
   and the in-progress row is gone.
4. **Wire `mode_payload` JSON serialization.** Same pattern as
   `PresetConfig`: core stores the blob opaquely, the shell owns
   the schema. Test round-trip via JSON string.

### Shell — `meditate-gtk` (estimate: 3–4 hours, 3–4 commits)

5. **Snapshot writes in `timer/imp.rs`.** Call
   `set_session_in_progress` on Session start; update on state
   transitions (pause, resume, mode-specific changes). Clear on
   normal completion inside the same transaction that calls
   `create_session`, so on_complete is atomic.
6. **60s GLib timer source while Running/Overtime.**
   `glib::timeout_add_seconds(60, ...)` that bumps
   `accumulated_secs`. Drop the source on completion, pause, or
   abandonment so we don't tick a stale session.
7. **Startup check + auto-finalize + toast.** In
   `application::startup`, call `get_session_in_progress`; if
   `Some`, run `finalize_session_in_progress`, invalidate, show
   the `adw::Toast` with Undo. Undo button calls `delete_session`
   on the inserted row.

### Verification

8. **Laptop test:** start a session, `pkill -9 meditate` after
   ~2 minutes, relaunch. Expect: session appears in log with
   correct duration, toast offers Undo, Undo deletes the row.
   Edge cases:
   - Start, pause, kill → relaunch finds in-progress with the
     pre-pause `accumulated_secs`.
   - Start a Guided session, kill, relaunch → finalized session
     shows the right guided file in the log card.
   - Start, complete normally, relaunch → no toast, no
     in-progress row.
9. **Librem 5 test:** the kill+relaunch flow on the phone using
   the standard deploy cycle. Confirms the auto-finalize path
   works on aarch64 + the toast renders correctly on Phosh.

## File-by-file change summary

- `meditate-core/src/db/schema.rs` — new table.
- `meditate-core/src/db/mod.rs` — declare the new submodule.
- `meditate-core/src/db/session_in_progress.rs` — new file with
  the struct + CRUD + finalize.
- `meditate-core/src/db/events.rs` (test) — verify
  `wipe_local_event_log` also wipes `session_in_progress` (it's
  a "remote data lost" primitive; in-progress state goes with it).
- `src/timer/imp.rs` — write snapshot on transitions, 60s tick,
  clear on completion.
- `src/application.rs` — startup check, auto-finalize, toast.
- `CORE_STRUCTURAL_BACKLOG.md` — remove the item from the "Up
  next" block and the larger Tier-1 eighth-pass entry.

## Open items deferred to v2

- **Resume option** in the recovery UX. If the auto-save proves
  insufficient ("I wanted to keep going from minute 49"), revisit.
- **Cross-device in-progress sync.** Currently device-local. A
  user pausing on the laptop and continuing on the Librem 5 is
  not in scope.
- **Two-instance racing.** Tracked separately under the existing
  audit; out of scope here.
