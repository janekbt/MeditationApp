# Design: one end screen and Auto-Finish

## Context
Auto-Finish adds a per-mode setting and a field in the preset JSON, both
of which sync. Everything else is UI and the session engine.

## Goals / Non-Goals
- Goal: older data and older devices keep working.
- Non-goal: a schema change or a new event kind.

## Decisions
- Settings: new keys `timer_auto_finish`, `guided_auto_finish`,
  `boxbreath_auto_finish` ("true"/"false", default false), next to
  `*_keep_screen_awake`. They sync through the existing `setting_changed`
  event like every other key. Rejected: one global key (Janek chose per
  mode).
- Preset JSON: `auto_finish: bool` with `#[serde(default)]` in
  `PresetConfig`. Captured from and applied to the mode's key exactly
  like `keep_screen_awake`. Rejected: leaving presets out (Janek chose
  to include it).
- Engine: `SessionSettings` gets `auto_finish: bool`. At the countdown's
  end (Timer, Guided length or file end through `enter_overtime`, Box
  Breath countdown) a session with it on goes to Stopped with the end
  bell and a new `Effect::AutoFinish { duration_secs }` (the planned
  length) instead of `EnterOvertime` / `EndBoxBreath`. The end bell is
  not cut: no `StopActiveSignals` in that effect list. Each app saves on
  `AutoFinish` through the same save path as the Save button, with whatever
  is in the note field (a note typed during a pause is kept).

## Risks / Trade-offs
- An older app version ignores the unknown preset field and the unknown
  setting key. If it re-saves (overrides) a preset, the field is dropped
  and Auto-Finish comes back off on every device after sync. Accepted
  by Janek.
- Older versions never read `*_auto_finish`; a session there goes into
  overtime as before.

## Migration Plan
None needed: no schema change. Proof that old data reads:
`a_preset_from_before_auto_finish_applies_it_off` parses a preset JSON
string as 26.10.1 writes it (copied by hand from a 26.10.1 database row,
without the field) and applies it.

## Open Questions
None.
