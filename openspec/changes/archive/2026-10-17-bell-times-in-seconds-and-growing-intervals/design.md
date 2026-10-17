# Design: bell times in seconds

## Context
`interval_bells` stores `minutes` and `jitter_pct`. Bells sync as
`interval_bell_insert/update` events whose JSON payload replaces the
whole row on apply (`recompute_interval_bell`); missing fields fall
back to defaults. Presets embed bells as JSON (`PresetIntervalBell`)
with `minutes` and `jitter_pct` required by serde. The published
release (26.10.1) reads both and must keep working while devices are
on mixed versions.

## Goals / Non-Goals
- Goal: seconds-exact time, jitter in seconds, a step in seconds,
  with no data loss for existing bells and no breakage on a release
  build that syncs with a new one.
- Non-goal: keeping a step or a seconds time alive through an edit on
  a release build (accepted by Janek).

## Decisions
- **Six added nullable columns** `secs`, `jitter_secs`, `step_secs`
  and their units `secs_unit`, `jitter_unit`, `step_unit` (`'s'` or
  `'min'`) via `ADDED_COLUMNS`, no fill SQL, no `SCHEMA_VERSION` bump.
  A NULL time reads as `minutes * 60` in min; a NULL jitter as
  `round(minutes * 60 * jitter_pct / 100)`, in min when whole minutes,
  else s; a NULL step as 0 s. A value in min is always a whole number
  of minutes. Payload and preset keys use the same names. Rejected: renaming the columns, which needs a table rebuild
  and locks out the release build after a downgrade.
- **Old columns and keys stay written.** Every write also stores
  `minutes = max(1, round(secs / 60))` and
  `jitter_pct = min(50, round(jitter_secs * 100 / secs))`, in the row
  and in the event and preset JSON, so a release build reads a
  sensible bell. Rejected: writing only the new keys, which makes a
  release build read every new bell as 0 minutes and fail to parse
  every new preset.
- **Rows written by an older build are detected on read**: when the
  stored `minutes` or `jitter_pct` differs from what this version would
  write for the stored seconds, an older build changed the row, so the
  values are derived from `minutes`/`jitter_pct` as for NULL. Apply of
  an event without the new keys writes the derived values. This closes
  the downgrade case below.
- **Bell rows are rebuilt from their events on every open**
  (`recompute_interval_bells_from_events`). A release build that
  applies a newer device's event keeps the whole payload in `events`
  but fills only `minutes` and `jitter_pct`; this happens on the
  device that updates second and on one that went back to the
  release. Rebuilding restores the seconds and step from the event.
  Only existing rows that have events are rebuilt (a deleted bell is
  already gone; a row without events is left as it is), so
  the cost is a few queries per bell in the library, not per bell
  ever synced.
  Rejected: a one-time fill or a `CACHE_SCHEMA_VERSION` bump, which
  run once and miss a device that goes back and forward again.
- **Rounding** of the old fields: half up, from the real seconds
  (150 s -> 3 min; 90 s +-30 s -> 33 %).
- **Apply prefers the new keys**: `secs`, `jitter_secs`, `step_secs`
  when present, else derived from `minutes`/`jitter_pct` as above and
  step 0. Preset bells hold the new fields as `Option`, filled from
  `minutes`/`jitter_pct` right after parsing, so `same_settings`
  compares seconds and units and an old preset still matches its
  library bell.
- **Schedule** keeps per-bell `base_secs`, `jitter_secs`, `step_secs`
  and the number of gaps so far; gap k = base + k * step; the next
  ring is the last ring plus gap k with jitter drawn in
  [gap - j, gap + j] (j capped at half the base time), at least 1 s. The time-jump catch-up loop
  steps the same way.
- **Title key** in core carries each time as (number, unit) in its
  stored unit; shells map it to their strings.

## Risks / Trade-offs
- A release build that edits a bell writes only `minutes` and
  `jitter_pct`: the new peer then shows the time rounded to minutes,
  the jitter from the percent, step 0. Accepted, README note.
- Going back to the release build and changing a bell there writes
  an event without the seconds keys. On the next open the rebuild
  applies it like every peer does: time in whole minutes, step 0, so
  the devices agree. The read check above is the backstop for a row
  read before that rebuild.
- A percent that has no exact seconds value changes slightly on the
  first write from this version: 1 min ±1 % reads as ±1 s, which
  older versions then see as 2 %. Accepted: the bell rings the same.

## Migration Plan
An existing database gets the six columns empty; every bell reads
exactly as before. Proven by a test that opens the
`schema_v26_10_1.sql` fixture with a bell of 7 min ±14 % and reads
7 min, ±59 s, step 0; and by a test that applies a release-build
event (no new keys) the same way.

## Open Questions
None.
