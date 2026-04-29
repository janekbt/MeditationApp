# Plan: Nextcloud sync (Option C — append-only event log)

## Goal

Periodic, automatic sync of session/label data between the user's
devices via a personal Nextcloud instance. Survives offline edits on
multiple devices without losing data, regardless of which device wrote
last.

Single-user-with-multiple-devices is the assumption: phone (Librem 5),
laptop, possibly future Android tablet. No multi-tenant, no sharing,
no merging across users.

## Why Option C over file-snapshot sync (B)

File-snapshot sync (upload .db snapshot, last-write-wins on the whole
file) loses data when offline edits happen on two devices. Option C
treats every mutation as a self-contained, addressable event; merging
two devices' event logs is well-defined regardless of timing. The
extra implementation cost buys robustness that the meditation use-case
actually benefits from (sessions on phone Sun/Tue/Thu, on laptop the
other days, both eventually syncing — common pattern).

## Architecture overview

### Events as source of truth

Every state-changing operation produces one **event**:

```
SessionInserted { uuid, start_iso, duration_secs, label_uuid, notes, mode }
SessionUpdated  { uuid, start_iso, duration_secs, label_uuid, notes, mode }
SessionDeleted  { uuid }
LabelInserted   { uuid, name }
LabelRenamed    { uuid, name }
LabelDeleted    { uuid }
SettingChanged  { key, value }
```

Plus envelope fields on every event: `lamport_ts`, `device_id`,
`event_id` (UUID), `kind`.

The local SQLite tables (`sessions`, `labels`, `settings`) are a
**materialized cache** derived from the event log. Reads hit the
cache; writes append an event AND update the cache atomically.

### Lamport clock for ordering

Each device keeps a monotonic logical counter:

```
on local event creation:    lamport = lamport + 1
on remote event observation: lamport = max(lamport, remote.lamport) + 1
```

Tie-break by lexicographic `device_id` UUID. This gives a total order
that all devices agree on after sync, regardless of wall-clock skew.

### Stable cross-device identity

Every session and label gets a `uuid` column (TEXT NOT NULL UNIQUE).
Generated at insert time via `uuid::Uuid::new_v4()`. The existing
SQLite `id INTEGER` rowid stays — it's the local cache's key. The
UUID is what crosses the network and survives between devices.

### Device identity

Each device generates a UUID once at first launch, stored in a
single-row `device` table. Never changes (even across DB resets;
preserved or regenerated as a clean device).

### Tombstones beat updates

Delete events are **tombstones** — applying a delete after an update
deletes; applying an update after a delete is a no-op. This avoids
the "edit + concurrent delete" resurrection bug.

## Schema additions

```sql
-- Add to existing tables:
ALTER TABLE sessions ADD COLUMN uuid TEXT NOT NULL UNIQUE;
ALTER TABLE labels   ADD COLUMN uuid TEXT NOT NULL UNIQUE;

-- New tables:

CREATE TABLE events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    event_uuid  TEXT NOT NULL UNIQUE,           -- stable across devices
    lamport_ts  INTEGER NOT NULL,
    device_id   TEXT NOT NULL,                  -- which device made it
    kind        TEXT NOT NULL,                  -- 'session_insert' etc.
    payload     TEXT NOT NULL,                  -- JSON
    synced      INTEGER NOT NULL DEFAULT 0      -- 0 = not yet uploaded
);
CREATE INDEX events_lamport ON events(lamport_ts, device_id);
CREATE INDEX events_synced  ON events(synced);

CREATE TABLE device (
    device_id     TEXT PRIMARY KEY,
    lamport_clock INTEGER NOT NULL DEFAULT 0
);

-- sync_state: server URL, app-password, last-synced cursor, etc.
-- Sensitive values (password) live in libsecret/Keystore, not here.
CREATE TABLE sync_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

## WebDAV layout

```
{user's NC root}/Meditate/
├── events/
│   ├── 00000000000001-{device_uuid}-{event_uuid}.json
│   ├── 00000000000002-{device_uuid}-{event_uuid}.json
│   └── ...
└── snapshot.json    (optional, for compaction)
```

Filenames sort chronologically (zero-padded lamport_ts as prefix), so
`PROPFIND` + sort gives the canonical replay order. Each event is a
separate file: avoids the contention/atomicity issues of appending to
a shared log file over WebDAV.

## Sync protocol

### Pull phase
1. `PROPFIND /Meditate/events/` — listing of all event filenames.
2. Diff against local: which event_uuids are remote-only?
3. For each new file: `GET`, parse JSON, validate.
4. Apply events in lamport order to materialized cache (idempotent —
   inserting an event whose UUID we already have is a no-op).
5. Update `lamport_clock` based on observed values.

### Push phase
1. `SELECT FROM events WHERE synced = 0 ORDER BY lamport_ts`.
2. For each: `PUT /Meditate/events/{filename}.json`.
3. On 201/204: mark row `synced = 1`.
4. On conflict (412 If-None-Match) or other error: skip, retry next sync.

### Transaction boundary
Pull is **non-destructive** (only adds events). Push only happens
after pull completes successfully. This gives "I've seen everything
you have, here's mine" semantics — devices converge eventually.

## Conflict resolution rules (with examples)

| Scenario | Resolution |
|---|---|
| Two devices insert different sessions | Both UUIDs unique → both kept. ✓ |
| Two devices update same session | Higher lamport_ts wins (tie: lexicographic device_id). |
| Update + delete on same session | Delete wins (tombstone). |
| Insert + delete on same session | If delete's lamport > insert's, deleted. Else still present (catches replay-out-of-order). |
| Same event observed twice | Idempotent — UUID UNIQUE on `events.event_uuid`, second insert is no-op. |
| Settings: two devices change same key | Higher lamport wins. |

## Implementation phases (TDD-friendly cycles)

### Phase A: schema + event recording (offline-only) — ~3 cycles
A1. Add `uuid` to sessions/labels (migration, populate via `uuid::Uuid::new_v4()` on insert)
A2. `events` + `device` + `sync_state` tables
A3. Wrap every mutation in `meditate_core::db::Database` to also append an event
- Tests: every CRUD path produces exactly one event with the right shape

### Phase B: event replay + dedup — ~2 cycles
B1. `apply_event(&Event)` function: idempotent, applies to materialized state
B2. `replay_events(events: &[Event])`: apply in lamport order, dedup on event_uuid
- Tests: arbitrary event sequence → expected state; double-apply same events; tombstone semantics; out-of-order delivery

### Phase C: WebDAV client — ~2 cycles
C1. Pure-Rust client in `meditate-core/src/sync/webdav.rs`: PROPFIND, GET, PUT, MKCOL, DELETE
C2. Auth via Basic-auth with app-password header
- Tests: against a mocked HTTP server (`mockito` or hand-rolled); verify headers, body, error mapping

### Phase D: sync orchestration — ~2 cycles
D1. `Sync::pull()` and `Sync::push()` against an injected `WebDav` trait
D2. `Sync::sync()` = pull then push, transactional
- Tests: simulated two-device scenario in-process. Insert on A, sync. Insert on B, sync. Both end up with both. Repeat with conflicts.

### Phase E: GTK shell wiring — ~2 cycles
E1. Settings dialog: NC URL, username, app-password, sync interval; "Test connection" + "Sync now" buttons
E2. Background scheduler: `glib::timeout_add_seconds` per the user's interval; status icon in headerbar (idle / syncing / error)
- Hand-tested on Librem 5 against your real Nextcloud

### Phase F: compaction + edge cases — ~1-2 cycles
F1. Periodic snapshot: dump materialized state as `snapshot.json`, delete events older than snapshot lamport_ts
F2. Initial-sync handling: detect "first time syncing this device, but I have local data" and generate events for existing rows
F3. Network-failure resilience: partial pull/push retries, no-internet detection

**Total estimate: ~12 cycles, plus device testing — call it 4-6 implementation sessions.**

## Open decisions

1. **Encryption of payloads.** Nextcloud's WebDAV stores plaintext.
   For session data this is probably fine (it's already on the user's
   own server). But if you want defense-in-depth, encrypt the JSON
   payload with a passphrase-derived key, store nothing sensitive in
   filenames. Adds ~1 cycle of crypto plumbing.

2. **App-password storage.** `secret-service` (libsecret) on Linux is
   the standard. Android Keystore on Android. Both have Rust crates
   (`secret-service`, `keystore`). Decide before Phase E.

3. **Sync trigger policy.** Options:
   - Every N minutes when app is open (simple, can miss closed-app changes)
   - On every session save + N-minute heartbeat
   - Manual only (button in settings)
   Recommend: on-save + 30-min heartbeat. Configurable.

4. **Event log retention before compaction.** A meditation session
   per day = ~365 events/year. After 5 years that's 1825 events,
   each ~500 bytes JSON = ~1 MB. Fine to never compact for personal
   use. Decide if Phase F is needed at all.

5. **What about settings sync?** `SettingChanged` events let you sync
   prefs (presets, daily goal, sound choice). Decide whether that's
   in scope or settings stay device-local. Probably in scope — annoying
   to set up presets twice.

## Out of scope

- Multi-user / shared sessions. Hard problem (auth, permissions, who
  can edit whose data). Not what this is for.
- Real-time sync. Eventual consistency over WebDAV is fine.
- Conflict UI. Resolution is fully automatic per the rules above; no
  user-facing "merge two timelines" prompt.
- Sync of the diagnostics log. It's per-device and gets cleared
  anyway; not data the user cares about syncing.

## References

- CRDT/event-sourcing primer: see e.g. James Long's "CRDTs for Mortals" talk
- Nextcloud WebDAV API: https://docs.nextcloud.com/server/latest/developer_manual/client_apis/WebDAV/index.html
- App passwords: Settings → Security → "Devices & sessions" on the user's NC instance
- Lamport clocks: Lamport, "Time, Clocks, and the Ordering of Events in a Distributed System" (1978)
