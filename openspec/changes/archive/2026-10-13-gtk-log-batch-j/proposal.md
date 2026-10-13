# GTK Log (batch J)

## Why
The Log on the laptop shows wrong rows and totals: yesterday stays
"Today" overnight (and Stats with it, on both apps), a filtered Log
shows a non-matching new session, the label filter shows "All labels"
while it filters, a note edit drops the start seconds, a delete
miscounts the day caption, and a deleted card comes back during Undo.
OPEN-ISSUES #117, #118, #119, #120, #121, #123.

## What Changes
- #117: Log and Stats notice a new calendar day within seconds, on
  their own, in both apps (also after the computer wakes up).
- #118: saving a session while a Log filter is on reloads the Log
  instead of adding the card.
- #119: the label filter always shows the label it filters by, with
  its current name; a deleted filter label clears the filter.
- #120: editing a session keeps its start seconds.
- #121: deleting a session lowers the day caption by the minutes its
  card showed.
- #123: a card waiting for Undo stays hidden through a Log rebuild and
  is counted once.
- Left out: the Timer page's streak line overnight (not in the list;
  a streak doesn't break at midnight). The "lived day" idea (the day
  ends at 3 a.m. for night owls) goes into OPEN-ISSUES as an idea: it
  changes how both apps count, not when the screen refreshes.

## Decisions
- The day changes at midnight (Janek, 2026-10-13).
- The visible page updates on its own (Janek, 2026-10-13).
- Android gets the #117 fix too (Janek, 2026-10-13).
- The check rides on loops that already run: GTK's 2 s window poll,
  Android's 200 ms tick. No new timer; a clock stopped by suspend
  catches up on the first poll after wake-up. (my call)
- On a new day both apps reload Log and Stats; GTK only rebuilds the
  page on screen, the other one on its next visit. (my call)
- The filter dropdown is rebuilt each time the filter popover opens,
  so renamed labels show their new names. (my call)

## Capabilities
### New Capabilities
- `stats`: Stats follows the calendar day.
### Modified Capabilities
- `log`: day headers, filters, edits and deletes show the right rows and totals.

## Impact
- Core: `time.rs` (a day watch both apps share).
- GTK: `log/imp.rs`, `log/mod.rs`, `window/imp.rs`.
- Android: `ui.rs` (day watch in the tick loop).
- Translations: none.
- Data: none (#120 writes the start it already had).
- Android build environment: untouched.
- Docs: OPEN-ISSUES.md (batch J ticked, "lived day" idea added).
