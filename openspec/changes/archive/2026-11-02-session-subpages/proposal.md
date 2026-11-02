# Session and Bells subpages

## Why
GitHub issue #20: every session setting sits on the main screen, so the
screen you start from is long and busy. A comment on it adds: after
changing one setting you should be able to start right there.

## What Changes
- Main screen keeps the time, the mode toggle, Start Session, the
  presets and the mode's own inputs (Box Breath phases, Guided file and
  starred files). The groups Session and Bells, and the Box Breath cues,
  leave it.
- Where the Session group is today (after the presets and the mode's
  own inputs), two rows open subpages:
  - "Session", subtitle: what differs from plain, joined by " · ":
    "Stopwatch" when on, the chosen label's name when a label is on,
    "Auto-Finish" when on and not with Stopwatch (it does nothing then).
    Nothing to say: no subtitle.
  - "Bells", subtitle: what rings, joined by " · ". Timer: "Starting
    Bell", "2 interval bells", "End Bell"; Box Breath: "End Bell",
    "Cues during phases"; Guided: "End Bell". None on: "No bells".
    It follows what will ring: with Stopwatch on there is no end bell
    and no interval bell timed from the end; Interval Bells on with no
    bell enabled, or phase cues on with every phase off, say nothing.
- Session page: the rows of today's Session group without Cues
  (Stopwatch Mode, Duration, Label, Auto-Finish, Keep Screen Awake),
  same per-mode rules.
- Bells page: Cues (sound, vibration or both) first, then Starting Bell
  (with Preparation Time), Interval Bells (Manage Bells), End Bell, then
  in Box Breath the cues per phase.
- A start bar at the bottom of Session, Bells and Manage Bells: the
  time as on the main screen ("00:20", "Hours · Minutes" under it in
  Timer and Box Breath), then a tall "Start Session" button, enabled
  exactly when the main screen's is (mockup 3 on both platforms). Linux:
  the page's background, a line above it only while content scrolls
  under it (libadwaita's flat bottom bar). Android: the standard bottom
  bar colour (surface container, as the navigation bar).
- Tapping the time, on the main screen and in the start bar, opens the
  Duration dialog (the one the Duration row opens). Not in Guided, not
  with Stopwatch on. The Duration row stays.
- Start from a subpage: the running screen slides in over it as it does
  over the main screen; the subpage closes unseen behind it, so Stop or
  Done returns to the main screen.
- Left out: keeping the bottom navigation bar visible on subpages (all
  subpages cover it today; Janek: leave it). Start bar in editors and
  choosers (unsaved edits, a choice in progress).

## Decisions
- Two rows with summaries plus a start bar, built together (Janek, 2026-11-02).
- Session summary shows "Stopwatch", the label's bare name and
  "Auto-Finish" (Janek); no subtitle when none (my call).
- The Cues row (sound / vibration / both) moves to the Bells page (Janek).
- Auto-Finish leaves the Session summary while Stopwatch is on, as the
  Bells summary follows what will happen (Janek, after the review).
- Bells page ends with End Bell, then the Box Breath cues (Janek).
- Presets stay on the main screen (Janek).
- Start from a subpage: running screen in first, subpage closes behind (Janek).
- Start bar: big time left, button right, mockup 3; Linux line only
  while content scrolls under it, Android bottom-bar colour (Janek).
- Start bar on Session, Bells and Manage Bells (Janek).
- Time tap opens the Duration dialog; Duration row stays (Janek).
- Summary pieces joined by " · " as elsewhere in the app, not ", "
  (zh_CN lists differently) (my call).
- The Bells summary follows what will ring, as the Manage Bells count
  and the greyed End Bell row do (my call).
- "Cues during phases" reused for the Box Breath piece, no new text (my call).

## Capabilities
### Modified Capabilities
- `setup` (new area): what the main screen and its subpages show

## Impact
- Core: the two summaries as typed keys (`preset_config.rs` next to
  `setup_visibility`), with tests
- GTK: `data/ui/timer_view.blp` (rows; groups move out), two new
  NavigationPages built like `bells.rs`'s, a start bar shared by the
  three pages, `timer/imp.rs` wiring, `window/imp.rs` start from a
  subpage (push running, then drop the subpage from the stack)
- Android: `ui/main.slint` (rows; two new slide-in pages like the
  Interval Bells library; start bar component; the running screen
  declared over them), `src/ui.rs` wiring
- Translations: new "1 interval bell"/"{n} interval bells" and "No
  bells", nine languages, both apps; everything else reuses texts
- Data: none
- Android build environment: untouched
- Docs: README screenshots show the old main screen (Janek retakes);
  README features list if it names the layout
