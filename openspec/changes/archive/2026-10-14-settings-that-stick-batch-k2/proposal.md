# Settings that stick (batch K2)

## Why
On the Timer page of both apps, a setting change that fails to save
(the database is busy with a sync, or the disk is full) is shown but not
stored: it is gone after a restart and never reaches the other device,
and nothing tells the user. OPEN-ISSUES #191 (laptop); the phone has the
same problem.

## What Changes
- A Timer page setting that fails to save shows "Save failed", and the
  page reloads its settings from the database, so the control shows the
  saved value again. Both apps.
- Covered: bell switches and bell picks (start, end, preparation,
  interval), preparation time, session length, Box Breath pattern,
  length and phase cues, cue type, stopwatch, keep screen awake, label
  on/off and label pick, bell volumes; on the phone also the preset star
  in the preset chooser and the accent colour in Preferences.
- Left out: the crash-recovery snapshot written while a session runs
  (not a setting the user made; its failure doesn't change what the
  page shows) and the sync indicator's retry reset (not a setting).

## Decisions
- Found while building: Save on the Done page also remembers the label as
  next time's default. When only that fails, the message is its own,
  "Label not kept for next time", since the session did save (Janek).
- The phone's accent colour in Preferences is covered too: "Save failed",
  the pick springs back (Janek).
- After a failed save the control springs back to the saved value
  (Janek: a failed value isn't saved later by a sync, so showing it
  would be wrong).
- The reload uses the functions the page already runs after a sync pull, not a
  per-control undo (my call: one place instead of about 50).
- The reload runs on the next main-loop turn, not inside the change
  handler that failed (my call: GTK spin rows and Slint sliders are
  mid-change there).
- A preset (or its Undo) that fails anywhere, its length included, shows "Save failed", no Undo, and the setup from before is applied again (Janek, after review R3: an Undo makes no sense for something that failed). Real all-or-nothing needs transactions in core: #193, batch K3.
- A volume that fails to save plays no preview (my call).
- Laptop bell volumes now save through the same write path as every
  other setting, which also starts a sync right away; today a volume
  change waits for the next sync (my call).

## Capabilities
### Modified Capabilities
- `timer`: a failed setting save says so and the page shows the saved value.

## Impact
- Core: none
- GTK: `src/timer/imp.rs` (one helper: `write_ok`, then reload on failure), `src/db/mod.rs` (scan test covers the Timer page)
- Android: `src/ui.rs` (the `write_*` setting helpers take the window, go through `write_ok`, reload on failure; preset star, accent, label default at Save), `ui/main.slint` (the new text), `src/app.rs` (test)
- Translations: "Label not kept for next time", new in both apps, nine languages each ("Save failed" exists)
- Data: none
- Android build environment: untouched
- Docs: OPEN-ISSUES.md (K2 done, #191 removed; #193 added as K3)
