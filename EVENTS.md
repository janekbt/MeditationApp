# Event catalogue

Wire-format spec for the events that flow between MeditationApp
peers over WebDAV sync. Authoritative reference for any peer not
built on meditate-core (a future iOS port, third-party
import/export tools).

For the higher-level model (event-sourced cache, recompute_*
materialisation, migration policy) see `ARCHITECTURE.md`.

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
| `payload`    | string (JSON)         | JSON-encoded event body. See per-kind shapes below. The envelope carries the payload as a string (JSON-in-JSON), so SQLite stores it without a JSON-aware projection. |

## Conflict resolution

For each `target_id`, the cache row materialised by `recompute_<entity>`
is determined by:

1. **Tombstone wins on tie or precedence.** If a `<entity>_delete`
   event exists with `lamport_ts ≥ max(lamport_ts)` of all
   `<entity>_insert / _update / _rename` events for the same
   `target_id`, the row is absent.
2. **Otherwise the latest mutate event wins.** Ordering is by
   `(lamport_ts DESC, device_id DESC)`. The lex-larger `device_id`
   wins on ties, the same on every peer, so two devices applying
   the same event set converge on the same outcome.

All fields in the winning mutate's payload are written into the
cache row. There are no per-field-LWW merges; the payload is taken
whole. Exceptions: a name may be conflict-suffixed (see Name
collisions); references are resolved to local ids; a missing field
takes the entity's default (listed per kind below).

A delete is not final. An Undo re-emits `_insert` for the same uuid
with a higher `lamport_ts`, and the row returns with its original
timestamps (`restore_preset`, `restore_guided_file`,
`restore_vibration_pattern`).

`setting_changed` and `box_breath_phase_update` have no tombstone:
the latest event wins.

### Name collisions

Labels, presets, guided files and vibration patterns have a
case-insensitive UNIQUE `name`. When two rows materialise with the
same name, the row with the lexically smaller uuid keeps it,
regardless of arrival order. The other row becomes
`<name> (conflict-<first 8 chars of its own uuid>)`. If the current
holder is the one that must yield, it is renamed in the cache
without emitting an event, because every peer makes the same move
(`claim_name` in `meditate-core/src/db/events.rs`).

After each replay, every row carrying its own uuid's suffix is
recomputed, so it takes its plain name back once the holder was
renamed or deleted (`reclaim_suffixed_names`).

Only labels raise a merge / keep-both prompt
(`list_label_conflicts`, `merge_labels`). Locally authored
duplicates are refused with `Duplicate*` errors and never suffixed.

## Forward compatibility

A peer MUST record events it does not understand. When a future
build adds dispatch for the new kind, it bumps `CACHE_SCHEMA_VERSION`
(`meditate-core/src/db/schema.rs`). `Database::init` sees the stored
value is lower, replays the whole event log and so picks up the
newly-understood kinds.

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

An event whose `lamport_ts` is negative or above i64::MAX/2 is not
even recorded.

## Atomicity on the wire

Each push uploads one file
`Meditate/events/<min_lamport, 14-digit zero-padded>__<batch_uuid>.json`
holding a JSON array of events. The upload goes to `.tmp` and is
MOVEd to the final name, so a partial body can't appear at the
final path. A file whose name doesn't split into exactly two parts
on `__` is skipped. The puller records the `batch_uuid` in
`known_remote_files` only after a successful replay, so the same
file is never replayed twice on the same peer.

Compaction: past 50 known batches, a device uploads its full log as
one batch, adds the swallowed batch_uuids to `Meditate/compacted.json`,
then deletes those batches. The event log itself is never pruned.

Audio files go to `Meditate/sounds/<uuid>.<ext>` and
`Meditate/guided/<uuid>.ogg`.

---

# Kind catalogue

Seven insert/update/delete families plus two update-only kinds
(`setting_changed`, `box_breath_phase_update`). `EventKind::entity()`
maps each kind to its table and `recompute_for` dispatches to it
(`meditate-core/src/db/events.rs`); `apply_event_inner` and
`replay_events` call both.

## Sessions

Recompute: `recompute_session`. `target_id` is the session uuid.

### `session_insert` / `session_update`

Same payload shape for both. `session_update` overrides every
field with the new value. There is no "patch" semantics.

```json
{
  "uuid":             "550e8400-e29b-41d4-a716-446655440001",
  "start_iso":        "2026-05-13T07:30:00",
  "duration_secs":    1500,
  "label_uuid":       "550e8400-e29b-41d4-a716-446655440002",
  "notes":            "Morning sit before standup",
  "mode":             "timer",
  "guided_file_uuid": null,
  "guided_file_name": null
}
```

- `start_iso`: local wall-clock at session start (note: local, not
  UTC; the shell that authored it stamped its local zone).
- `duration_secs`: non-negative `u32` cast to JSON number.
- `label_uuid`: uuid of the label row, or `null`. Resolved to a
  local `label_id` at apply time via the `labels` cache; if the
  label hasn't arrived yet the session materialises with
  `label_id = NULL` and gets re-linked when its `label_insert`
  later arrives (see `recompute_label` re-link logic). A writer
  whose label is gone locally saves the session with `null`
  instead of failing (`session_label`).
- `notes`: string or `null`. Writers send `null` for an empty or
  whitespace-only note.
- `mode`: one of `timer`, `box_breath`, `guided` (missing means
  `timer`). Any other value fails the CHECK constraint; see
  [Version skew](#version-skew).
- `guided_file_uuid`: uuid of the guided file row when
  `mode == "guided"`, else `null`. Carried in the payload so
  per-file stats stay consistent across devices. Also `null` for a
  guided file played without importing it. Once a file is deleted,
  its name for the Log comes from the last `guided_file_insert` /
  `_update` event that carried `name`.
- `guided_file_name`: title of the file a guided session played, as
  the Guided page showed it (library, Local Folder or Open File), else
  `null`. The Log shows the library name when `guided_file_uuid`
  resolves, otherwise this. Optional: older writers omit it.
  The one exception to full overwrite: when the winning event has no
  name, the newest event of the session that has one supplies it, so
  an older peer's edit doesn't erase it.

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

- `name`: case-insensitive UNIQUE; see
  [Name collisions](#name-collisions).

### `label_rename`

```json
{
  "uuid": "550e8400-e29b-41d4-a716-446655440002",
  "name": "Focus"
}
```

Same shape and same effect as `label_insert`: both are mutates, and
the latest one sets the name.

### `label_delete`

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440002" }
```

Sessions that referenced this label survive with `label_id = NULL`
(FK is `ON DELETE SET NULL`).

`merge_labels` emits one `session_update` per re-tagged session, a
`setting_changed` for any `default_label_uuid_*` key that pointed at
the merged label, and then `label_delete`, all in one transaction.

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
  "created_iso":  "2026-05-13T07:30:00.123456789+00:00",
  "updated_iso":  "2026-05-13T07:30:00.123456789+00:00"
}
```

- `name`: case-insensitive UNIQUE; see
  [Name collisions](#name-collisions).
- `mode`: one of `timer`, `box_breath`, `guided` (missing means
  `timer`).
- `config_json`: JSON string stored as given; the schema is
  `PresetConfig` in `meditate-core/src/preset_config.rs`. Missing
  means `{}`.
- `is_starred`: bool. Whether the preset appears in the visible
  chip list above the Save / Manage buttons.
- `created_iso` / `updated_iso`: RFC 3339 in UTC, written by
  chrono as `+00:00` with fractional seconds. `updated_iso` bumps on
  rename / config-change / star-toggle.

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
  "created_iso": "2026-05-13T07:30:00.123456789+00:00"
}
```

`bell_sound_update` is emitted only by a rename.

- `is_bundled`: bundled sounds ship with the app and are seeded
  at first launch; the UI gates `is_bundled = true` rows from
  delete to keep them from being removed by mistake.
- `mime_type`: `"audio/wav"` or `"audio/ogg"` (the importer
  transcodes everything else to OGG on import). Missing means
  `audio/wav`.
- `category`: `general` (starting/interval/end bells) or
  `box_breath` (phase cues). Missing means `general`. The two are
  exclusive on purpose. The Box Breath chooser never falls back to
  general sounds; with no `box_breath` sound it shows an empty
  state. An imported sound takes its category from the chooser it
  was imported from.
- `file_path`: the authoring device's path. Peers store it but
  never use it. Custom audio lives at `<sounds_dir>/<uuid>.<ext>`
  on every device and is fetched by uuid; bundled audio comes from
  the shell's built-in table.

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
  "minutes":                 2,
  "jitter_pct":              33,
  "secs":                    90,
  "secs_unit":               "s",
  "jitter_secs":             30,
  "jitter_unit":             "s",
  "step_secs":               0,
  "step_unit":               "s",
  "sound_uuid":              "550e8400-e29b-41d4-a716-446655440004",
  "vibration_pattern_uuid":  "550e8400-e29b-41d4-a716-446655440006",
  "signal_mode":             "sound",
  "enabled":                 true,
  "created_iso":             "2026-05-13T07:30:00.123456789+00:00",
  "volume":                  50
}
```

- `kind`: one of `"interval"`, `"fixed_from_start"`,
  `"fixed_from_end"`. Missing means `interval`.
- `secs`: fire cadence (interval) or offset (fixed-from-*), in
  seconds. `jitter_secs`: an interval ring lands up to this many
  seconds early or late (capped at half of `secs` when it rings).
  `step_secs`: each interval gap is the previous one plus this.
  Each has a `*_unit` (`secs_unit`, `jitter_unit`, `step_unit`),
  `"s"` or `"min"`, the unit it was set in and shows in; in
  `"min"` the value is a whole number of minutes.
- `minutes` and `jitter_pct`: what builds before seconds read,
  still written for them: `secs` in whole minutes rounded half up
  (at least 1), and `jitter_secs` as a percent of `secs` rounded
  half up (at most 50). A payload whose seconds fields are missing,
  or no longer round to these two (a build before seconds changed
  the bell), applies from `minutes` and `jitter_pct`, with no step.
  So a build before seconds that edits a bell, switches it on or
  off or applies a preset to it resets its step and rounds its
  time to whole minutes.
- `sound_uuid`: `bell_sounds.uuid`. Missing or `null` applies as
  the bundled singing bowl.
- `vibration_pattern_uuid`: `vibration_patterns.uuid`. Missing or
  `null` applies as the bundled Pulse pattern.
- `signal_mode`: one of `"sound"`, `"vibration"`, `"both"`. Any
  other value fails the table's CHECK constraint (see
  [Version skew](#version-skew)).
- `enabled`: bool. Disabled bells stay in the row set but are
  filtered out of scheduling. Missing means true.
- `volume`: the bell's volume in percent, 0-100 in steps of 5
  (`meditate_core::bell_volume`). Other numbers are clamped to
  0-100 and rounded to the nearest step; a missing or non-numeric
  value applies as 50. A new bell is always inserted at 50. A
  build that doesn't know the field drops it when editing, so the
  volume falls back to 50.

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
  "created_iso":       "2026-05-13T07:30:00.123456789+00:00",
  "updated_iso":       "2026-05-13T07:30:00.123456789+00:00"
}
```

- `name`: case-insensitive UNIQUE; see
  [Name collisions](#name-collisions).
- `intensities_json`: JSON-encoded `[f32; N]`, values in `[0.0,
  1.0]`. Sampled at uniform time intervals across `duration_ms`.
- `chart_kind`: `line` (linear interpolation) or `bar`
  (sample-and-hold). It changes the playback curve. Missing means
  `line`.
- `is_bundled`: true for seeded patterns; UI gates them from
  delete.

### `vibration_pattern_delete`

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440006" }
```

## Guided files

Recompute: `recompute_guided_file`. `target_id` is the file uuid.

Files in a Guided Local Folder are not guided files and write no
event: their stars, lengths and play counts stay on the device. A
session played from one syncs with `guided_file_uuid` null and the
file's title in `guided_file_name`, like an Open File pick.

### `guided_file_insert` / `guided_file_update`

```json
{
  "uuid":          "550e8400-e29b-41d4-a716-446655440007",
  "name":          "Body scan, 20 min",
  "file_path":     "/var/.../meditate/guided/550e8400-….ogg",
  "duration_secs": 1200,
  "is_starred":    true,
  "created_iso":   "2026-05-13T07:30:00.123456789+00:00",
  "updated_iso":   "2026-05-13T07:30:00.123456789+00:00"
}
```

- `name`: case-insensitive UNIQUE; see
  [Name collisions](#name-collisions).
- `file_path`: same as bell sounds. Peers store it but never use
  it; the audio lives at `<guided_dir>/<uuid>.ogg` and is fetched by
  uuid.
- `duration_secs`: decoded once at import; cached so the Setup
  view doesn't re-decode on every render.
- `is_starred`: starred files are listed directly on the Setup
  Guided list; unstarred ones appear only in Manage Imports.

### `guided_file_delete`

```json
{ "uuid": "550e8400-e29b-41d4-a716-446655440007" }
```

## Box-breath phases

Recompute: `recompute_box_breath_phase`. `target_id` is the phase
name (one of `in`, `holdin`, `out`, `holdout`).

### `box_breath_phase_update`

There is no `_insert` or `_delete`. The four rows are seeded at
app start (`Database::seed_all_non_audio`) and persist for the life
of the DB; `wipe_local_event_log` re-seeds them. Seeding emits no
events. With no event, a row keeps its seeded defaults. Applying
an event for a phase that has no row yet creates it. Per-phase
configuration changes (which sound plays, which vibration fires,
whether the phase emits a signal at all) ride this event.

```json
{
  "phase":        "in",
  "enabled":      true,
  "signal_mode":  "both",
  "sound_uuid":   "550e8400-e29b-41d4-a716-446655440004",
  "pattern_uuid": "550e8400-e29b-41d4-a716-446655440006",
  "volume":       50
}
```

- `phase`: one of `in`, `holdin`, `out`, `holdout`.
- `enabled`: bool. Missing means false.
- `signal_mode`: one of `"sound"`, `"vibration"`, `"both"`, as
  for interval bells.
- `sound_uuid` / `pattern_uuid`: uuids of bell_sound /
  vibration_pattern rows. Missing or `null` applies as the bundled
  singing bowl / Pulse pattern.
- `volume`: the cue's volume in percent; same rules as
  `interval_bell_*`'s `volume`.

## Settings

Recompute: `recompute_setting`. `target_id` is the setting key.

### `setting_changed`

```json
{
  "key":   "daily_goal_mins",
  "value": "20"
}
```

- `key`: most keys are defined in meditate-core
  (`settings_keys.rs`, `goal.rs`), e.g. `daily_goal_mins`,
  `timer_session_secs`, `breathing_in`, `default_label_uuid_timer`.
- `value`: string. Numbers are stringified by the writer; the
  reader (e.g. `Database::get_setting("daily_goal_mins", "0")`)
  parses on demand with a caller-supplied default.

There is no `setting_delete`. Writing the value a key already holds
emits nothing. The one-shot seed flags (`default_labels_seeded`,
`bundled_bell_sounds_seeded`, `default_presets_seeded`,
`bundled_vibration_patterns_seeded`) are ordinary synced settings.

---

# Implementation pointers

If you're writing a peer shell:

- **Cache as pure function of events.** A local change writes its
  cache row and appends its event in one transaction. The row must
  equal what `recompute_<entity>` would build from the log. A
  rename to the stored name emits nothing, because it would
  override a newer peer rename. (Exception for now: saving an
  unchanged vibration pattern in the editor still emits
  `vibration_pattern_update`; OPEN-ISSUES #184.)
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
  traversal / phase-string validator from `target_id_is_well_formed_for`
  (`meditate-core/src/db/error.rs`).

Reference Rust implementation lives in
`meditate-core/src/db/events.rs` (`apply_event_inner`,
`replay_events`) and the per-entity `recompute_*` functions in
each `meditate-core/src/db/<entities>.rs` file (e.g. `sessions.rs`,
`settings.rs`).

## Version skew

Events are applied with last-write-wins semantics and unknown
*fields* in a payload are ignored, but an event whose enum-like
value (mode, category, kind, signal_mode, chart_kind)
violates an older build's SQLite CHECK constraints fails to apply
on that device: sync surfaces an error and stops ingesting until
the app updates. A payload that isn't valid JSON fails the batch
the same way. This is the intended failure mode: fail loudly
rather than materialise rows the old build can't interpret.
Nothing is lost: the batch stays on the remote and applies cleanly
after the update.
