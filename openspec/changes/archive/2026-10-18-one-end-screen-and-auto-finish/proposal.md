# One end screen and Auto-Finish

## Why
GitHub #6: ending a session takes two screens and two taps (Finish on
the running page, then Save on the Done page), and a countdown that
runs out keeps counting until someone taps. The Done page only adds
the note and a label pick that Setup already holds. The reporter wants
the session to end and save by itself.

## What Changes
- The Done page goes away in both apps. The running page ends every
  session:
  - Running or preparing: [Pause] [Discard]
  - Paused: [Resume] [Save], below it [Discard], below that the note field
  - Countdown or guided file ran out (overtime): [Save] [Add X ?], below
    it [Discard], below that the note field. Save keeps the planned
    length, Add keeps the overtime, as Finish and Add do today.
  - Box Breath countdown ended by itself: [Save] [Discard], below that the
    note field.
- Discard ends the session without saving. With a note typed it asks
  first ("Discard Session?"), as the Done page does today.
- A saved session gets the label set in Setup. The label switch and
  pick at the end are gone, and Save no longer rewrites the label for
  next time (Setup already saved it).
- New "Auto-Finish" switch in each mode's session options, next to the
  label, with the subtitle "Saves the session without asking when the time is up". It
  shows only for countdowns (Timer countdown, Guided, Box Breath
  countdown), not for a stopwatch. When on and the time is up: the end
  bell rings, the session is saved with its planned length, the app goes
  back to Setup and shows "Session saved" with Undo. Undo deletes it.
- Presets save and load Auto-Finish like "Keep screen awake".
- Left out: nothing of the request. Not added: an option to turn off
  logging altogether (the issue offers it as an alternative; Auto-Finish
  covers the need).

## Decisions
- Pause: [Resume] [Save], [Discard] below (Janek).
- Running: Stop becomes Discard, ends without saving, asks first only
  with a note (Janek).
- Box Breath end: [Save] [Discard] in one row, note below (Janek).
- Auto-Finish: in the session options, per mode, with a subtitle (Janek).
- Auto-Finish end: bell, save at planned length, back to Setup, "Session
  saved" with Undo (Janek).
- Presets carry Auto-Finish (Janek). An older app version that re-saves
  the preset drops it to off on sync; accepted.
- Wording: "Auto-Finish" / "Saves the session without asking when the time is up" /
  "Session saved"; German "Automatisch beenden" / "Speichert die Sitzung
  ohne Nachfrage, wenn die Zeit um ist" / "Sitzung gespeichert" (Janek).
- Which buttons show in which state is decided in core; both apps draw
  them (my call).
- Linux matches the phone: Add moves into the row next to Save, Discard
  below (my call).
- Android Back on the running page still does nothing in every state, so
  nothing is lost by a stray swipe; Linux Esc unchanged (my call).
- The note is shown when paused, in overtime and after a Box Breath end,
  hidden while running; its text is kept across Resume (my call).
- The GTK Setup "Paused" layout (Resume/Stop under the running page) is
  deleted if it can't be reached, otherwise its Stop becomes Save (my
  call, checked while building).
- Undo of an auto-saved session uses the same delete as the recovery
  Undo, and a failed Undo shows "Delete failed" in both apps (my call).
  The guided file's play count stays, as when deleting from the Log.
- Auto-Finish saves the note if one was typed during a pause (my call).
- Paused during the preparation time: [Resume] [Discard] only, no Save,
  so preparation seconds are never saved as meditation (my call).
- A session that ended but failed to save shows [Save] [Discard] and the
  note, in every mode (my call).
- Linux Space only pauses and resumes; in overtime and after the end it
  does nothing, so one key never saves (my call).
- The Auto-Finish switch shows for Guided in every display mode, since a
  file always ends; with count-up display it ends without a bell, as the
  end bell is already off there (my call).
- A guided file that ends before its stored length saves the stored
  length, as Finish does today (my call).
- The "Meditation complete" notification is still sent when the window is
  unfocused or the phone in the background (my call). "Session saved"
  with Undo shows only while the app is in view; after a background end
  the session is simply in the Log (my call).
- A preset counts as active regardless of Auto-Finish while the switch is
  hidden (stopwatch), like the other hidden settings (my call).

- After the device test (Janek, 2026-10-18): Add reads "Add {time} and
  Save" (German "{time} hinzufügen und speichern", my wording), on two
  lines if needed; the Auto-Finish row is greyed out, not hidden, for a
  stopwatch; the laptop's open Log follows the Undo at once; the phone's
  Manage Imports and Local Folder sit side by side again, two lines
  allowed (one house wrapping button, since the bundled Material buttons
  clip and are build environment).

## Capabilities
### Modified Capabilities
- `timer`: ending a session, Auto-Finish
- `labels`: scenarios that named the Done page
- `presets`: Auto-Finish in a preset

## Impact
- Core: `session/` (button set per state, Auto-Finish ends instead of
  overtime), `settings_keys.rs` (per-mode key), `preset_config.rs`,
  `seeds.rs`
- GTK: `src/window/imp.rs` (running page), `src/timer/imp.rs` (Done
  logic moves to the running page, Auto-Finish row), `data/ui/timer_view.blp`
  (Done page removed, Auto-Finish row)
- Android: `ui/main.slint` (Done overlay removed, running page buttons and
  note, Auto-Finish row), `src/ui.rs`, `src/app.rs` (AppState Finished
  becomes "ended on the running page")
- Translations: new "Auto-Finish", "Saves the session when the time is
  up", "Session saved", in both apps, nine languages each. "Save",
  "Discard", "Resume", "Note (optional)", "Discard Session?" exist. GTK's
  untranslated running-page "Pause" and "Stop and Save Session" are
  replaced by translated strings.
- Data: no schema change. New per-mode settings `{timer,guided,boxbreath}_auto_finish`
  (synced through the existing `setting_changed`), new optional field
  `auto_finish` in preset JSON (serde default false). See design.md.
- Android build environment: untouched
- Docs: README features, ARCHITECTURE session engine section
