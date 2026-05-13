//! Version sentinels + the SQL schema string applied at DB init.

/// On-disk schema version. Bumped when the SQL in `SCHEMA` changes in
/// a way that an older build cannot read safely. A DB whose
/// `PRAGMA user_version` exceeds this constant is rejected at open
/// time to prevent a downgrade from silently corrupting forward-only
/// data.
pub const SCHEMA_VERSION: u32 = 1;

/// Cache materialization version. Bumped when `apply_event_inner`
/// learns a new event kind it previously recorded-but-skipped, or
/// changes the cache columns a kind materialises. On open, if the
/// stored value is less than this constant, every event in the log
/// is re-applied so historical events for newly-understood kinds
/// land in the cache. Stored in `sync_state` (local-only) rather
/// than `settings` (event-sourced) so peers don't see each other's
/// cache progress as something to sync.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// `sync_state` key holding the device-local cache schema version.
pub const CACHE_SCHEMA_VERSION_KEY: &str = "cache_schema_version";

pub(super) const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS labels (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL COLLATE NOCASE UNIQUE,
        uuid TEXT NOT NULL UNIQUE
    );
    -- Audio-file library referenced by every bell-fire site (starting
    -- bell, interval bells, completion sound). is_bundled rows ship
    -- with the app and use a GResource path in file_path; user-
    -- imported custom rows (B.5) point at $XDG_DATA_HOME/.../sounds/
    -- and ride sync as actual files (B.6). The seed-on-first-run
    -- path inserts bundled rows with stable hardcoded UUIDs so a
    -- peer device that already has the bundle doesn't end up with
    -- duplicate rows after a sync round-trip.
    CREATE TABLE IF NOT EXISTS bell_sounds (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid        TEXT NOT NULL UNIQUE,
        name        TEXT NOT NULL,
        file_path   TEXT NOT NULL,
        is_bundled  INTEGER NOT NULL DEFAULT 0,
        mime_type   TEXT NOT NULL,
        category    TEXT NOT NULL DEFAULT 'general'
                    CHECK (category IN ('general', 'box_breath')),
        created_iso TEXT NOT NULL
    );
    -- User-managed library of bells fired during a Timer-mode session.
    -- Three kinds (see IntervalBellKind): periodic with jitter, fixed
    -- offset from start, fixed offset from end. `enabled` is the
    -- per-row checkmark — disabled rows stay in the library but don't
    -- ring. `sound` mirrors the existing bowl/bell/gong vocabulary
    -- and transitions to a UUID into the bell-sound library in B.4.
    -- `created_iso` is captured at insert and never updated; it lets
    -- list views sort newest-first or oldest-first without an extra
    -- column on the row.
    CREATE TABLE IF NOT EXISTS interval_bells (
        id                     INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid                   TEXT NOT NULL UNIQUE,
        kind                   TEXT NOT NULL CHECK (kind IN ('interval', 'fixed_from_start', 'fixed_from_end')),
        minutes                INTEGER NOT NULL,
        jitter_pct             INTEGER NOT NULL DEFAULT 0,
        sound                  TEXT NOT NULL DEFAULT 'bowl',
        -- Default uses the bundled Pulse pattern's stable UUID
        -- (BUNDLED_PATTERN_PULSE_UUID in src/db/mod.rs). Kept literal
        -- here to avoid plumbing a shell-side const into the core
        -- schema string.
        vibration_pattern_uuid TEXT NOT NULL DEFAULT '7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001',
        signal_mode            TEXT NOT NULL DEFAULT 'sound'
                               CHECK (signal_mode IN ('sound', 'vibration', 'both')),
        enabled                INTEGER NOT NULL DEFAULT 1,
        created_iso            TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS sessions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        start_iso TEXT NOT NULL,
        duration_secs INTEGER NOT NULL,
        label_id INTEGER REFERENCES labels(id) ON DELETE SET NULL,
        notes TEXT,
        mode TEXT NOT NULL CHECK (mode IN ('timer', 'box_breath', 'guided')),
        uuid TEXT NOT NULL UNIQUE,
        -- Guided meditation rows that played a library-stored file
        -- (an entry in guided_files) carry the file uuid here so
        -- per-file stats can resolve later. NULL for non-Guided rows
        -- AND for transient one-off guided sessions where the user
        -- played a file without importing it into the library.
        guided_file_uuid TEXT
    );
    -- Named, full-fidelity session templates. `config_json` is opaque
    -- to core (the shell defines its schema). `mode` is mirrored out
    -- of the JSON into a column so the visible-list query can filter
    -- by mode without JSON parsing. `is_starred` is the per-preset
    -- pin into the home-screen chip list. Both bundled rows (seeded
    -- by the shell on first open) and user-created rows live here
    -- with no `is_bundled` distinction — every preset is fully
    -- editable (rename / restar / delete) per the design spec.
    CREATE TABLE IF NOT EXISTS presets (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid        TEXT NOT NULL UNIQUE,
        name        TEXT NOT NULL COLLATE NOCASE UNIQUE,
        mode        TEXT NOT NULL CHECK (mode IN ('timer', 'box_breath', 'guided')),
        is_starred  INTEGER NOT NULL DEFAULT 0,
        config_json TEXT NOT NULL,
        created_iso TEXT NOT NULL,
        updated_iso TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS presets_mode_idx ON presets(mode);
    -- Guided-meditation audio library. Each row is a user-imported
    -- track that the app transcoded to OGG and stored under the
    -- per-device data dir. `is_starred` is the per-row pin into the
    -- home-screen list; the chooser shows every row regardless.
    -- `name` is COLLATE NOCASE UNIQUE so the user can't end up with
    -- two rows that look the same in the chooser. `duration_secs` is
    -- denormalised here so the home-screen subtitle and the hero
    -- countdown can render without re-probing the file.
    CREATE TABLE IF NOT EXISTS guided_files (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid          TEXT NOT NULL UNIQUE,
        name          TEXT NOT NULL COLLATE NOCASE UNIQUE,
        file_path     TEXT NOT NULL,
        duration_secs INTEGER NOT NULL,
        is_starred    INTEGER NOT NULL DEFAULT 0,
        created_iso   TEXT NOT NULL,
        updated_iso   TEXT NOT NULL
    );
    -- User-managed library of vibration patterns. Each row is a full
    -- envelope (duration + N equally-spaced amplitude samples + chart
    -- kind) referenced by per-bell signal config and box-breath phase
    -- rows. Bundled rows ship with the app under stable UUIDs so peer
    -- devices with the bundle don't end up with duplicate rows after a
    -- sync round-trip. `name` is COLLATE NOCASE UNIQUE so the chooser
    -- can't show two visually identical entries. `chart_kind` is
    -- persisted because Line and Bar describe two different playback
    -- semantics (linear interpolation vs. sample-and-hold step) — same
    -- intensities, different output curve.
    CREATE TABLE IF NOT EXISTS vibration_patterns (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid             TEXT NOT NULL UNIQUE,
        name             TEXT NOT NULL COLLATE NOCASE UNIQUE,
        duration_ms      INTEGER NOT NULL,
        intensities_json TEXT NOT NULL,
        chart_kind       TEXT NOT NULL DEFAULT 'line'
                         CHECK (chart_kind IN ('line', 'bar')),
        is_bundled       INTEGER NOT NULL DEFAULT 0,
        created_iso      TEXT NOT NULL,
        updated_iso      TEXT NOT NULL
    );
    -- Per-phase cue config for Box Breath. Always exactly four rows
    -- (one per phase id), seeded on first open. Shape mirrors
    -- per-bell signal config: enabled + signal_mode + sound_uuid +
    -- pattern_uuid. No insert / delete operations — only updates.
    -- sound_uuid default points at BUNDLED_BOWL_UUID; pattern_uuid
    -- default at BUNDLED_PATTERN_PULSE_UUID. Both are kept literal
    -- here to avoid plumbing shell-side consts into the core schema
    -- string.
    CREATE TABLE IF NOT EXISTS box_breath_phases (
        phase        TEXT PRIMARY KEY
                     CHECK (phase IN ('in', 'holdin', 'out', 'holdout')),
        enabled      INTEGER NOT NULL DEFAULT 0,
        signal_mode  TEXT NOT NULL DEFAULT 'sound'
                     CHECK (signal_mode IN ('sound', 'vibration', 'both')),
        sound_uuid   TEXT NOT NULL DEFAULT 'f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001',
        pattern_uuid TEXT NOT NULL DEFAULT '7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001'
    );
    CREATE TABLE IF NOT EXISTS settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    -- Single-row per database. Holds the stable per-device UUID that
    -- tags every locally-authored event in the sync log, plus the
    -- monotonic Lamport counter used to order events across devices.
    -- `lamport_clock` defaults to 0 and is bumped on local writes /
    -- max-merged on remote observations.
    CREATE TABLE IF NOT EXISTS device (
        device_id     TEXT PRIMARY KEY,
        lamport_clock INTEGER NOT NULL DEFAULT 0
    );
    -- Append-only event log for Nextcloud sync. Every row is a
    -- self-contained description of a state-changing operation. Reads
    -- (replay, push) sort by `lamport_ts` for causal ordering;
    -- `event_uuid` UNIQUE makes append idempotent against retries and
    -- peer-forwarded duplicates. `synced` is the push-queue gate.
    -- `target_id` denormalises the affected row identity (session or
    -- label uuid, or setting key) so replay queries can scan all
    -- events for one target via an index instead of JSON parsing.
    CREATE TABLE IF NOT EXISTS events (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        event_uuid  TEXT NOT NULL UNIQUE,
        lamport_ts  INTEGER NOT NULL,
        device_id   TEXT NOT NULL,
        kind        TEXT NOT NULL,
        target_id   TEXT NOT NULL,
        payload     TEXT NOT NULL,
        synced      INTEGER NOT NULL DEFAULT 0
    );
    -- Index on (lamport_ts, device_id) supports the canonical
    -- replay-order scan; SQLite tie-breaks on device_id so the order is
    -- deterministic across peers.
    CREATE INDEX IF NOT EXISTS events_lamport_idx
        ON events(lamport_ts, device_id);
    -- Index on `synced` makes `pending_events` (which scans WHERE
    -- synced = 0) efficient even when the log grows large.
    CREATE INDEX IF NOT EXISTS events_synced_idx
        ON events(synced);
    -- Index on `target_id` makes the apply_event recompute query
    -- (all events touching one uuid/key) fast even when the log has
    -- thousands of entries.
    CREATE INDEX IF NOT EXISTS events_target_idx
        ON events(target_id);
    -- Sync-loop bookkeeping: server URL, last-pull cursor, last
    -- successful sync timestamp, etc. Separate namespace from `settings`
    -- so user-facing prefs and sync internals don't share a key space.
    -- Sensitive values (app password) belong in libsecret/Keystore, not
    -- here.
    CREATE TABLE IF NOT EXISTS sync_state (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    -- Filename-level dedup for the bulk-file sync layout. Each remote
    -- file has a `batch_uuid` baked into its name; the puller records
    -- batch_uuids it has already ingested here so a subsequent pull
    -- can skip GET on files it already replayed (events themselves are
    -- still dedup'd by event_uuid via `events`, but this avoids the
    -- per-file GET round-trip). The pusher records its own batch_uuid
    -- here on success so we don't re-fetch our own uploads.
    CREATE TABLE IF NOT EXISTS known_remote_files (
        file_uuid TEXT PRIMARY KEY
    );
    -- Per-bell tracker for the bell-sound audio files synced over
    -- WebDAV (B.6). Mirrors known_remote_files but keyed on the
    -- bell_sounds.uuid rather than the bulk-file batch_uuid — each
    -- bell sound is its own remote file, with its own PUT/GET cycle.
    -- The push side INSERT-OR-IGNOREs into this table after a
    -- successful PUT; the pull side checks membership before
    -- issuing a GET to skip files this device already pulled or
    -- pushed itself.
    CREATE TABLE IF NOT EXISTS known_remote_sounds (
        bell_uuid TEXT PRIMARY KEY
    );
    -- Singleton table holding the current in-flight meditation
    -- session's running state so a crash / OOM / battery-death
    -- mid-session doesn't lose the work the user already did.
    -- Lives OUTSIDE the event log: writes here do not call
    -- emit_event, so sync sees nothing while a session is in
    -- progress. The shell upserts on session start + tick boundaries
    -- (~60s cadence); on the next launch core's finalize_session_in_progress
    -- reads the row, emits one session_insert event with the
    -- captured accumulated_secs, and clears the row — same code
    -- path a normal end-of-session takes. Single-row CHECK keeps
    -- the table honest about the at-most-one-in-flight-session
    -- invariant. mode_payload is opaque JSON the shell defines
    -- (mirrors the PresetConfig convention).
    CREATE TABLE IF NOT EXISTS session_in_progress (
        id               INTEGER PRIMARY KEY CHECK (id = 1),
        start_iso        TEXT    NOT NULL,
        accumulated_secs INTEGER NOT NULL,
        mode             TEXT    NOT NULL
                         CHECK (mode IN ('timer', 'box_breath', 'guided')),
        mode_payload     TEXT    NOT NULL,
        label_id         INTEGER REFERENCES labels(id) ON DELETE SET NULL,
        guided_file_uuid TEXT
    );
";
