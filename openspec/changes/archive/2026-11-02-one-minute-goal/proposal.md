# One-minute goal

## Why
GitHub issue #19: a daily goal under 5 minutes can't be set, though
1 minute a day is a real goal for someone starting out.

## What Changes
- The smallest daily goal is 1 minute in both apps (was 5); the largest
  stays 1440.
- Linux: the goal's +/- buttons step by 1 (was 5), Page Up/Down by 5.
- Android: the Daily Goal dialog accepts 1; its hint reads
  "Minutes (1–1440)" by itself, no new text.
- Left out: a minimum of 0 (a zero goal breaks the ring math and reads
  as the default today).

## Decisions
- Linux steps by 1, Page Up/Down by 5 (Janek, 2026-11-02).
- The old comment's reason for 5 ("every practised day lands in the top
  heatmap level") doesn't hold: the levels are shares of the goal, so a
  1 minute goal reaches the top level at 2 minutes. The comment goes
  (my call).

## Capabilities
### Modified Capabilities
- `stats`: the daily goal can be 1 minute

## Impact
- Core: `goal.rs` limit and step
- GTK: `src/preferences.rs` page step and hold speed stay 5
- Android: none in code (bounds come from core); the Slint preview
  default `goal-min-mins` follows to 1
- Translations: none
- Data: none; the goal setting already syncs any positive number
- Android build environment: untouched
- Docs: none name the limit
