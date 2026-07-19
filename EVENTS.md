# Event catalogue

Wire-format spec for the events that flow between MeditationApp
peers over WebDAV sync. Authoritative reference for any non-Rust
peer shell (Android Kotlin/Java port, future iOS, third-party
import/export tooling).

For the higher-level model — event-sourced cache, recompute_*
materialisation, no-migrations policy — see `ARCHITECTURE.md`.

## Envelope

Every event, serialised to JSON, has the same outer shape:

```json
{
  "event_uuid": "550e8400-e29b-41d4-a716-446655440000",
  "lamport_ts": 137,
  "device_id":  "00000000-0000-4000-8000-aaaaaaaaaaaa",
  "kind":       "session_insert",
  "target_id":  "550e8400-e29b-41d4-a716-446655440001",
  "payload":    "{\"uuid\":\"550e8400-…\",\"start_iso\":\"…\",…}"
}
```

| Field        | Type                  | Notes                                                                                                      |
| ------------ | --------------------- | ---------------------------------------------------------------------------------------------------------- |
| `event_uuid` | string (v4 uuid)      | Idempotency key. Receiving the same `event_uuid` twice is a no-op on the cache (the events table dedups).  |
| `lamport_ts` | int64                 | Local Lamport clock at the authoring device when the event was emitted. Used for last-write-wins.          |
| `device_id`  | string (v4 uuid)      | Stable identity of the authoring device. Used as tiebreaker for events emitted with the same `lamport_ts`. |
| `kind`       | string discriminant   | One of the kinds catalogued below. Unknown kinds MUST be recorded but not dispatched (forward-compat).     |
| `target_id`  | string                | Cross-device identity of the affected entity. UUID for most; setting key for `setting_changed`; phase name for `box_breath_phase_update`. Denormalised onto the row so replay queries don't have to parse the JSON payload. |
| `payload`    | string (JSON)         | JSON-encoded event body. See per-kind shapes below. Note: this is JSON-in-JSON — the outer envelope serialises the payload as a string. Trade-off was made for SQLite simplicity (no JSON-aware projection needed) at the cost of a slightly uglier wire. |

## Conflict resolution

For each `target_id`, the cache row materialised by `recompute_<entity>`
is determined by:

1. **Tombstone wins on tie or precedence.** If a `<entity>_delete`
   event exists with `lamport_ts ≥ max(lamport_ts)` of all
   `<entity>_insert / _update / _rename` events for the same
   `target_id`, the row is absent.
2. **Otherwise the latest mutate event wins.** Ordering is by
   `(lamport_ts DESC, device_id DESC)`. The lex-larger `device_id`
   wins on ties — consistent across all peers per the plan's
   tie-break rule, so two devices applying the same event set
   converge on the same outcome.

All fields in the winning mutate's payload are written into the
cache row. There are no per-field-LWW merges; the payload is taken
whole.

## Forward compatibility

A peer MUST record events it does not understand. When a future
build adds dispatch for the new kind, `Database::init` walks the
event log on cache-schema bump and rebuilds the cache from scratch,
picking up the newly-understood kinds.

The recorded row keeps its `event_uuid`, so the future build's
re-pull is also idempotent.

## target_id validation

A receiving peer rejects any event whose `target_id` contains a
path separator (`/`, `\`), a null byte, or is empty. For
`box_breath_phase_update`, `target_id` must be one of `in`,
`holdin`, `out`, `holdout`. Rejected events are still recorded
(forwards-compat), but skip dispatch. This prevents a peer from
shipping a `bell_sound_insert` with
`target_id = "../../../etc/passwd"` and getting the puller to
write attacker-chosen bytes outside the sounds dir.

## Atomicity on the wire

`Sync::push` bundles all pending events into one JSON file per
push batch, named `<batch_uuid>.json` under the sync folder. The
file is uploaded `.tmp` then MOVE'd to the canonical name so a
partial body can't appear at the canonical path. The puller's
`known_remote_files` table records the `batch_uuid` after
successful replay — the same file is never replayed twice on the
same peer.

---

# Kind catalogue

There are nine entity recompute families plus two singletons
(`setting_changed`, `box_breath_phase_update`). The dispatch table
lives in `apply_event_inner` (`meditate-core/src/db/events.rs`).

## Sessions

Recompute: `recompute_session`. `target_id` is the session uuid.

### `session_insert` / `session_update`

Same payload shape for both — `session_update` overrides every
field with the new value. There is no "patch" semantics.

```json
{
  "uuid":             "550e8400-e29b-41d4-a716-446655440001",
  "start_iso":        "2026-05-13T07:30:00",
  "duration_secs":    1500,
  "label_uuid":       "550e8400-e29b-41d4-a716-446655440002",
  "notes":            "Morning sit before standup",
  "mode":             "timer",
  "guided_file_uuid": null
}
```

- `start_iso` — local wall-clock at session start (note: local, not
  UTC; the shell that authored it stamped its local zone).
- `duration_secs` — non-negative `u32` cast to JSON number.
- `label_uuid` — uuid of the label row, or `null`. Resolved to a
  local `label_id` at apply time via the `labels` cache; if the
  label hasn't arrived yet the session materialises with
  `label_id = NULL` and gets re-linked when its `label_insert`
  later arrives (see `recompute_label` re-link logic).
- `notes` — string or `null`. `null` and empty string are distinct;
  don't collapse on import.
- `mode` — one of `"timer"`, `"box_breath"`, `"guided"`. The SQL
  CHECK constraint includes all three; a peer emitting a fourth
  value will be recorded but not materialise.
- `guided_file_uuid` — uuid of the guided file row when
  `mode == "guided"`, else `null`. Carried in the payload so
  per-file stats stay consistent across devices.

### `session_delete`

Tombstone. Payload carries the uuid for replay correctness:

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440001" }
```

## Labels

Recompute: `recompute_label`. `target_id` is the label uuid.

### `label_insert`

```json
{
  "uuid": "550e8400-e29b-41d4-a716-446655440002",
  "name": "Morning"
}
```

- `name` — case-insensitive UNIQUE in the `labels` table. On a
  sync-merge collision (two peers offline both name a label
  "Morning") `recompute_label` retries the second row under a
  uuid-suffixed name (`Morning (conflict-550e8400)`) so sync
  doesn't hard-stall on the poison event.

### `label_rename`

```json
{
  "uuid": "550e8400-e29b-41d4-a716-446655440002",
  "name": "Focus"
}
```

Same shape as `label_insert`; the discriminant lets the apply path
distinguish "this row is appearing for the first time" (insert
hasn't been seen yet) from "this row was renamed."

### `label_delete`

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440002" }
```

Sessions that referenced this label survive with `label_id = NULL`
(FK is `ON DELETE SET NULL`).

## Presets

Recompute: `recompute_preset`. `target_id` is the preset uuid.

### `preset_insert` / `preset_update`

```json
{
  "uuid":         "550e8400-e29b-41d4-a716-446655440003",
  "name":         "Morning sit",
  "mode":         "timer",
  "is_starred":   true,
  "config_json":  "{\"duration_secs\":900, …}",
  "created_iso":  "2026-05-13T07:30:00Z",
  "updated_iso":  "2026-05-13T07:30:00Z"
}
```

- `mode` — one of `"timer"`, `"box_breath"`, `"guided"`.
- `config_json` — opaque-to-core JSON string. The shell owns the
  schema (see `meditate-core::preset_config`).
- `is_starred` — bool. Whether the preset appears in the visible
  chip list above the Save / Manage buttons.
- `created_iso` / `updated_iso` — RFC 3339 UTC. `updated_iso`
  bumps on rename / config-change / star-toggle.

### `preset_delete`

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440003" }
```

## Bell sounds

Recompute: `recompute_bell_sound`. `target_id` is the sound uuid.

### `bell_sound_insert` / `bell_sound_update`

```json
{
  "uuid":        "550e8400-e29b-41d4-a716-446655440004",
  "name":        "Tibetan bowl",
  "file_path":   "/var/.../meditate/sounds/550e8400-….ogg",
  "is_bundled":  false,
  "mime_type":   "audio/ogg",
  "category":    "general",
  "created_iso": "2026-05-13T07:30:00Z"
}
```

- `is_bundled` — bundled sounds ship with the app and are seeded
  at first launch; the UI gates `is_bundled = true` rows from
  delete to keep them from being removed by mistake.
- `mime_type` — `"audio/wav"` or `"audio/ogg"` (the importer
  transcodes everything else to OGG on import).
- `category` — one of `"general"`, `"interval"`, `"start_end"`.
- `file_path` — absolute path on the **authoring** device. Peers
  ignore this field — the actual audio bytes are fetched
  separately via `pull_custom_sound_files` keyed on `uuid`.
  Bundled rows use a gresource path; custom rows live under
  `<data>/meditate/sounds/<uuid>.<ext>`.

### `bell_sound_delete`

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440004" }
```

## Interval bells

Recompute: `recompute_interval_bell`. `target_id` is the bell uuid.

### `interval_bell_insert` / `interval_bell_update`

```json
{
  "uuid":                    "550e8400-e29b-41d4-a716-446655440005",
  "kind":                    "interval",
  "minutes":                 5,
  "jitter_pct":              0,
  "sound":                   "550e8400-e29b-41d4-a716-446655440004",
  "vibration_pattern_uuid":  "550e8400-e29b-41d4-a716-446655440006",
  "signal_mode":             "sound",
  "enabled":                 true,
  "created_iso":             "2026-05-13T07:30:00Z"
}
```

- `kind` — one of `"interval"`, `"fixed_from_start"`,
  `"fixed_from_end"`.
- `minutes` — fire cadence (interval) or offset (fixed-from-*).
- `jitter_pct` — 0-100, jitter applied to interval cadence to
  reduce predictability of cadence-anchored attention.
- `sound` — bell_sound uuid (or a legacy free-text key for
  pre-event-sourced rows; the legacy path is still tolerated for
  forwards compat but no new emit writes it).
- `vibration_pattern_uuid` — `vibration_patterns.uuid`, or `null`
  for sound-only bells.
- `signal_mode` — one of `"sound"`, `"vibration"`,
  `"sound_and_vibration"`.
- `enabled` — bool. Disabled bells stay in the row set but are
  filtered out of scheduling.

### `interval_bell_delete`

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440005" }
```

## Vibration patterns

Recompute: `recompute_vibration_pattern`. `target_id` is the
pattern uuid.

### `vibration_pattern_insert` / `vibration_pattern_update`

```json
{
  "uuid":              "550e8400-e29b-41d4-a716-446655440006",
  "name":              "Slow pulse",
  "duration_ms":       2000,
  "intensities_json":  "[0.0,1.0,0.0,1.0,0.0]",
  "chart_kind":        "bar",
  "is_bundled":        false,
  "created_iso":       "2026-05-13T07:30:00Z",
  "updated_iso":       "2026-05-13T07:30:00Z"
}
```

- `intensities_json` — JSON-encoded `[f32; N]`, values in `[0.0,
  1.0]`. Sampled at uniform time intervals across `duration_ms`.
- `chart_kind` — one of `"bar"`, `"line"`. Editor preview shape;
  doesn't affect playback.
- `is_bundled` — true for seeded patterns; UI gates them from
  delete.

### `vibration_pattern_delete`

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440006" }
```

## Guided files

Recompute: `recompute_guided_file`. `target_id` is the file uuid.

### `guided_file_insert` / `guided_file_update`

```json
{
  "uuid":          "550e8400-e29b-41d4-a716-446655440007",
  "name":          "Body scan — 20 min",
  "file_path":     "/var/.../meditate/guided/550e8400-….ogg",
  "duration_secs": 1200,
  "is_starred":    true,
  "created_iso":   "2026-05-13T07:30:00Z",
  "updated_iso":   "2026-05-13T07:30:00Z"
}
```

- `file_path` — same caveat as bell sounds: authoring-device-local
  absolute path; peers ignore it and fetch bytes via the sync
  pipeline keyed on `uuid`.
- `duration_secs` — decoded once at import; cached so the Setup
  view doesn't re-decode on every render.
- `is_starred` — whether the file appears at the top of the Setup
  Guided picker.

### `guided_file_delete`

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440007" }
```

## Box-breath phases

Recompute: `recompute_box_breath_phase`. `target_id` is the phase
name (one of `in`, `holdin`, `out`, `holdout`).

### `box_breath_phase_update`

There is no `_insert` or `_delete` — the four rows are seeded at
`Database::init` and persist for the life of the DB. Per-phase
configuration changes (which sound plays, which vibration fires,
whether the phase emits a signal at all) ride this event.

```json
{
  "phase":        "in",
  "enabled":      true,
  "signal_mode":  "sound_and_vibration",
  "sound_uuid":   "550e8400-e29b-41d4-a716-446655440004",
  "pattern_uuid": "550e8400-e29b-41d4-a716-446655440006"
}
```

- `phase` — one of `in`, `holdin`, `out`, `holdout`.
- `signal_mode` — one of `"sound"`, `"vibration"`,
  `"sound_and_vibration"`.
- `sound_uuid` / `pattern_uuid` — uuids of bell_sound /
  vibration_pattern rows, or `null` if `signal_mode` makes that
  channel inapplicable.

## Settings

Recompute: `recompute_setting`. `target_id` is the setting key.

### `setting_changed`

```json
{
  "key":   "weekly_goal_secs",
  "value": "9000"
}
```

- `key` — opaque to core; the shell owns the namespace (see
  `meditate-core::settings_keys`). Examples:
  `weekly_goal_secs`, `last_used_label_id`,
  `breath_phase_secs_in`, etc.
- `value` — string. Numbers are stringified by the writer; the
  reader (e.g. `Database::get_setting("weekly_goal_secs", "0")`)
  parses on demand with a caller-supplied default.

There is no `setting_delete`; clearing a setting writes an empty
string.

---

# Implementation pointers

If you're writing a peer shell:

- **Cache as pure function of events.** Do not let users mutate
  the cache directly; always go through an `emit_event` →
  `recompute_<entity>` path so divergent devices converge on the
  same materialisation.
- **Idempotency on `event_uuid`.** Your local `events` table
  should have `UNIQUE(event_uuid)`. `INSERT OR IGNORE` on
  conflict.
- **Bump local Lamport on receive.** When applying a foreign
  event, advance your local clock to `max(local, remote) + 1`
  before any subsequent local authorship.
- **Skip dispatch on unknown kind.** Record the row; the next
  cache-schema bump's walk-replay will pick it up when your build
  understands the kind.
- **Skip dispatch on malformed `target_id`.** Apply the path-
  traversal / phase-string validator from `target_id_is_well_formed_for`.

Reference Rust implementation lives in
`meditate-core/src/db/events.rs` (`apply_event_inner`,
`replay_events`) and the per-entity `recompute_*` functions in
each `meditate-core/src/db/<entity>.rs` file.

## Version skew

Events are applied with last-write-wins semantics and unknown
*fields* in a payload are ignored, but an event whose enum-like
value (mode, category, kind) violates an older build's SQLite
CHECK constraints fails to apply on that device: sync surfaces an
error and stops ingesting until the app updates. This is the
intended failure mode — fail loudly rather than materialise rows
the old build can't interpret. Nothing is lost: the batch stays
on the remote and applies cleanly after the update.
