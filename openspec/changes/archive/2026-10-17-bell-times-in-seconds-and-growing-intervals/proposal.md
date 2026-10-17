# Bell times in seconds, jitter as a time, growing intervals

## Why
Interval bells only take whole minutes, and their jitter is a percent
of the interval: "every 7 min ±14%" instead of "every 7 min ±1 min"
(GitHub #7). There is also no way to let the gap between rings grow,
e.g. 60 s, then 65 s, then 70 s (GitHub #5). Both were asked for by a
user on GitHub; Janek wants both, with every time settable in seconds
or minutes.

## What Changes
- An interval bell's time (for all three kinds: repeating, from start,
  before end) is set as a number plus a unit, seconds or minutes.
- The jitter of a repeating bell is a time (number plus unit), not a
  percent: the ring lands anywhere from time minus jitter to time plus
  jitter.
- A repeating bell gets a step (number plus unit, default 0): each gap
  is the one before it plus the step. With jitter, the jitter is drawn
  around the grown gap.
- Row titles show the units: "Every 120 s ±1 min, +5 s",
  "At 90 s", "30 s before end", "Every 5 min".
- README notes that all devices that sync should be updated together.
- Left out: a shrinking step (negative), since nobody asked; a cap on
  how far the gap may grow (a timer session ends anyway, a stopwatch
  bell just rings less often).

## Decisions
- Input: each of time, jitter and step is a number with a seconds/
  minutes switch beside it; stored in seconds (Janek).
- Each value keeps the unit it was set in: 60 s stays "60 s" in the
  editor and the title, and syncs that way (Janek). A bell from before
  this change opens in minutes; a jitter converted from percent opens
  in minutes when it is whole minutes, else in seconds.
- The base time of all three kinds gets the unit switch, not only the
  repeating kind (Janek).
- The jitter field replaces the percent field, no "range" shown under
  percent (Janek).
- An older version that changes a bell resets its step to 0 and its
  time to whole minutes; that includes switching it on or off and
  applying a preset there. That is fine, and README tells people to
  update every device together (Janek).
- Wording to approve (English source, both apps, 9 translations each):
  - title: "Every {time}", "At {time}", "{time} before end", with
    " ±{time}" for jitter and ", +{time}" for the step; a time reads
    "{n} min" or "{n} s"
  - the kind "Every N minutes" (laptop) / "Every N min" (phone) becomes
    "Repeating"
  - the row "Minutes" becomes "Time"; "Jitter" keeps its name, its
    subtitle becomes "Random shift of each ring"
  - new row "Step", subtitle "Added to the gap after each ring"
  - the switch reads "s" and "min"
- Limits: time 5 s to 120 min; jitter 0 to half the time (today's 50 %
  cap); step 0 to 60 min. (my call)
- Flipping the unit keeps the number and changes its meaning: 5 with
  s flipped to min is 5 min, not 0.08 min. (my call)
- Flipping to min where the number is over the unit's limit clamps
  it to the limit (200 s flipped to min becomes 120 min), as the
  steppers already clamp typed numbers. (my call)
- The jitter cap (half the time) applies when the bell rings and in
  its title, not to the stored value, so lowering the time and raising
  it again in the laptop's live-saving editor loses nothing. (my call)
- The step and the jitter apply to the previous ring as rung, as the
  jitter does today: next ring = last ring + gap k +- jitter. (my call)
- Only the repeating kind has jitter and step; switching to a fixed
  kind saves both as 0, in both apps (core decides). (my call)
- Titles reuse the existing per-kind templates and add seconds forms
  of them ("Every {n} s", "At {n} s", "{n} s before end", with plural
  forms); jitter and step are appended as " ±{n} min" / ", +{n} s"
  with the unit strings "{n} min" and "{n} s". (my call)
- Existing bells keep exactly their times; a percent jitter becomes
  seconds rounded to the nearest second (7 min ±14 % becomes ±59 s).
  (my call)

## Capabilities
### Modified Capabilities
- `bells`: interval bell times in seconds or minutes, jitter as a
  time, growing step.

## Impact
- Core: `bells.rs` (scheduling, limits, title key), `db/interval_bells.rs`
  (fields, events, apply), `db/schema.rs` (six added columns),
  `preset_config.rs` (preset bell fields), every `insert_interval_bell`
  caller and test fixture.
- GTK: `src/bells.rs` (editor rows, title), `src/db/mod.rs`.
- Android: `ui/main.slint` (interval editor, title strings),
  `src/ui.rs` (load, save, title), `src/app.rs` (clamps, tests).
- Translations: the strings above, both apps, 9 languages each; the
  old percent title strings go.
- Data: six added columns and new event and preset fields, old ones
  still written; no schema version bump; bell rows are rebuilt from
  their events on every open. See design.md.
- Android build environment: untouched.
- Docs: README (feature line, update note), EVENTS.md (interval bell
  payload), openspec bells spec. The fastlane descriptions ("fixed,
  periodic, or randomised") and the timer spec stay true unchanged.
