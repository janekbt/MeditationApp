# Tasks: Session and Bells subpages (GitHub #20)

Base: d5f6d7ac6ac3e7a6f6ac5e3226436b4eac6ebfe3; suites on it: core 1439,
android --lib 286, gtk 167 (the #19 commit's tested tree)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`,
      `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-02, with: Auto-Finish in the summary, Cues row to Bells, Linux flat bottom bar, Android bottom-bar colour)
- [x] 3 Build
- [x] 4 Review (one fresh agent, 7 findings; fix review PASS; /simplify below)
- [x] 5 Specs, docs, list (check-specs passes; README features don't name the layout, its screenshots show the old main screen: Janek retakes; #20 is a GitHub issue)
- [x] 6 Verify and hand over (Janek: both apps passed, Russian not tried, 2026-11-02)
- [x] 7 Ship

## Decisions so far (Janek, 2026-11-02)
- Main screen keeps time, mode toggle, Start, presets and the mode's own
  inputs; groups Session, Bells (and Box Breath cues) move to two
  subpages, reached by two rows "Session" and "Bells" with a summary.
- Summaries: only what differs from the default; the label shows as its
  bare name (no "Label" word), plus "Stopwatch" when on. Bells: what is on.
  Summary rules live in core.
- Bells page: Starting Bell, Interval Bells, End Bell, then below it the
  Box Breath cues (Box Breath mode only).
- Presets stay on the main screen.
- Start from a subpage: the running session slides in first, the
  subpage closes behind it, no page leaving then another arriving.
- Both parts together (subpages and the start bar), no sticky-only step.
- Start bar on the subpages: variant B, the time left, the button right;
  look: mockup 3 on both platforms (big time as on the main screen,
  "Hours · Minutes" under it, a tall pill button), no separator line
  at the top edge, same background as the page.
  Mockups: https://claude.ai/artifact/HF7gqAqU4LzPRLcDTBuhFy

- Subpages keep covering the whole app, bottom bar included, as every
  subpage does today (Janek: leave it, no issue for now).
- Tapping the time opens the Duration dialog, on the main screen and in
  the start bar (Janek, new: today only the Duration row opens it).
  Not in Guided (the file sets the length) or with Stopwatch on.

- Start bar on Session, Bells and Manage Bells; not in editors or
  choosers (Janek).
- Duration row stays on the Session page; the time tap is a shortcut (Janek).

## Pre-build questions (fresh agent), answered (my calls)
- Which pages close on start -> in `push_running_page` (every start path,
  Space included) drop only Session, Bells and Manage Bells; editors and
  choosers stay as today (Stop returns to them, no unsaved edit lost).
- Bells summary by stored switches or by what rings -> by what rings
  (Stopwatch kills the end bell and end-timed interval bells; empty
  interval or cue sets say nothing); new scenario and test.
- GTK pages in Rust or blueprint -> two `Adw.NavigationPage`s as extra
  objects in `timer_view.blp`; the rows stay template children, so the
  existing wiring and source-reading tests keep working; the window
  pushes them on `nav_view`.
- Row placement -> where the Session group is today, after the mode's
  inputs, in all three modes.
- Android z-order -> the Running screen and the Duration dialog move
  after the new pages and the Interval Bells library; Label and bell
  choosers stay above the pages; Android starts from the widget deep
  link close the new pages too (`close_transient_overlays`).

## Review Focus
- A preset applied, or a sync landing, while a subpage is open -> the
  page's rows and the main rows' summaries refresh like the setup does
  today (the same reload path); the start bar's time follows.
- A label deleted on another device while it is the chosen one -> the
  summary drops it as the label row does (core reads the stored label).
- Switching mode with a subpage open -> not possible: the mode toggle
  is only on the main screen.
- Starting while a bell preview is ringing on the Bells page -> the
  preview stops as when leaving the page today.
- Guided with no file -> Start disabled in the bar as on the main
  screen; the bar shows no time ("--" as the main screen does today).
- Back gesture or Escape while the running screen slides in -> the
  stack is already [main, running]; Back from running acts as today.
- Keyboard and screen reader -> the time is a real button with the
  Duration dialog's name; rows are activatable as today.
- German and Russian at 360 px -> summaries wrap, never cut (scenario).

## Review findings (fresh agent)
- 1 GTK summaries didn't follow a sync pull or closing Preferences
  (both call only `refresh_streak`) -> fixed: `refresh_streak` refreshes
  them; the extra calls in `reload_setup` and `apply_config_once` went
  (both call `refresh_streak`). Check `setup_summaries_follow_a_sync_and_
  leaving_a_page_stops_a_preview`, seen failing with the call removed.
- 2 GTK label name went unescaped into a markup subtitle ("Sit & Walk")
  -> fixed: `glib::markup_escape_text`.
- 3 Leaving the GTK Bells page didn't stop a Volume preview -> fixed in
  the pages' `hidden` handler (same check as 1).
- 4 "Auto-Finish" shows with Stopwatch on, where it does nothing ->
  Janek: leave it out; scenario and test changed first (failed with
  AutoFinish listed), then core.
- 5 " · " join and "No bells" fallback live in both apps -> not changed:
  text joining is the apps' job, like every other subtitle; the parts
  and the empty case come from core.
- 6 Android time button's accessible name replaced the time -> fixed:
  label is the time, description "Duration".
- 7 GTK time tap's targetability had no initial sync -> fixed: set once
  from the Duration row, then on its notify.
- Suites after the fixes: gtk 168, android 287, core 1444; clippy clean.
- Fix review (fresh agent): PASS. Notes: the vendored Android backend
  has no accessibility bridge, so fix 6 reaches no screen reader yet
  (correct, untestable on the FP5); fixes 2 and 7 have no own check
  (kept: one-line calls, a check would restate them); the 300-byte
  window in the new check stays (the `body` helper ends at a method's
  close, not a closure's).

## /simplify (4 agents)
- Applied: Android builds the two subtitles in Rust through the `Tr`
  catalogue, like `preset_subtitle` (reuse, simplification and altitude
  all flagged the bool struct + Slint join chain); `SetupSummary` and
  four Slint functions gone. GTK: misplaced doc comment back on
  `intervals_count_subtitle`; the summary refresh in
  `refresh_interval_bells_count` dropped (leaving the Bells page
  refreshes). Core: one `vis` in `bells_summary`. Slint: the close
  Timer's 300 ms is tied to the Running slide in its comment. A stray
  blank line in the blp. Suites again: core 1444, android 287, gtk 168;
  clippy clean host and aarch64.
- Skipped: one Slint page-shell component for the three pages (a
  refactor of existing overlays, no shell exists to reuse); one Rust
  GTK page builder instead of blueprint (decided before the build: the
  rows stay template children); the tag list on the GTK stack (Space
  starts from any page, so `push_running_page` is the one place);
  moving "can the time be tapped" to core (both apps follow their
  Duration row, which core already rules); fewer DB reads in
  `bells_summary` (it reads what the session engine reads, on purpose);
  merging the summaries into `refresh_streak`'s batch, the double
  refresh on session end and on Android pull (a few settings reads,
  not on a hot path); the text formatters into `preset_subtitle.rs`;
  dropping the window's `push_page`/`timer_view` wrappers.

## Tasks
- [x] 1 Core tests for the session and bells summaries (five scenarios),
      watch them fail; typed keys `SessionSummaryPart` / `BellsSummaryPart`
      next to `setup_visibility`; pass; whole core suite (3 failed on
      stubs; the two empty cases passed on the stub, proven by breaking
      the interval count and Auto-Finish check; core 1444)
- [x] 2 Android test `setup_summaries_are_translated` (two new msgids in
      all nine catalogs and as @tr in main.slint), watch it fail (failed:
      main.slint lacks @tr("No bells")); nine Android translations added
- [x] 3 GTK: rows on the main screen, Session and Bells NavigationPages
      holding today's rows, start bar on them and on Manage Bells, time
      as a button opening the Duration dialog, start from a subpage
      (push running, then drop the subpage); new texts, nine
      translations, `update-translations.sh`, i18n test (template test
      failed before the update; gtk 167 after; the rows on the pages sit
      outside the TimerView tree, so `setting_saved` also finds the view
      through the window)
- [x] 4 Android: rows, two slide-in pages, start bar component on them
      and on the Interval Bells library, time tap, running screen over
      the pages, subpage hidden after its slide; nine translations;
      task 2's test passes (android 287; host and aarch64 clippy clean;
      built by a helper agent from this folder). Its calls: page back
      buttons clear the flag, one `setup-page-closed` handler stops a
      bell preview and refreshes; a 300 ms Slint Timer closes the pages
      once Running covers them; the bar always shows "Hours · Minutes"
      as the Android hero does; rows reuse `ChooserRow`; four
      source-reading tests follow the new layout (header count 15,
      Running block now ends at the Duration dialog). Running is now
      declared after the app bar: on Stop it slides out over it.
- [x] 5 Whole suites, clippy both, msgfmt; builds for the MANUAL scenarios (GTK debug 21:47, debug APK 21:48, on the FP5)

## After the first device test (Janek, 2026-11-02)
- All passed, Russian not tried. Linux start button too tall (the
  46px min-height came on top of the pill's padding) -> 26px content,
  46px in all. Android button fine. Then the Linux bar itself was too
  tall: its margins 14 -> 6 px. Auto-Finish rule changed (above); suites
  core 1444, gtk 168, android 287; GTK 22:01, APK 22:01 on the FP5.

## Test steps (step 6)
Artifacts: `meditate-gtk/builddir/src/meditate`,
`meditate-android/android/app/build/outputs/apk/debug/app-debug.apk`.
1. Main screen, both apps: below presets and mode inputs two rows,
   Sitzung and Glocken, with summaries; no Session/Bells groups left.
2. Sitzung page: rows without Hinweise; Glocken page: Hinweise first,
   Startglocke, Intervallglocken, Endglocke, in Box Breath the cues.
3. Change something on a page, go back: the summary follows (label
   name, Stoppuhr, Automatisch beenden; Glocken by what rings, "Keine
   Glocken" when nothing).
4. Start bar on Sitzung, Glocken, Glocken verwalten: big time left,
   tall button right. Linux: line above it only while content scrolls
   under it. Android: bottom-bar colour. German and Russian at 360 px:
   nothing cut off.
5. Tap the time (main screen and bar): Duration dialog in Timer and
   Box Breath; nothing in Guided or with Stopwatch on.
6. Start from each of the three pages: Running slides in over the
   page; Stop or Done lands on the main screen. Linux: Space too.
7. Android: Running now slides out over the app bar on Stop.

## Commit message
setup: Move session and bell settings to their own pages

The main screen only shows what you start from; Session and Bells
open pages with a summary on their row, and every one of those
pages can start the session. A tap on the time sets the duration.
Fixes #20.
