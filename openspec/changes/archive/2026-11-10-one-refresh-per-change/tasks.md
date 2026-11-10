# Tasks: one-refresh-per-change

Base: 183249a45366f8641611760ebe4f8aad7e91cf05; suites on it: core 1543 + 18, GTK 170, Android 299 green (sync-after-real-writes' final runs on this exact code; the commit only added specs and the archive). Lines at base: 61,764.

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (one `.slint` callback signature, glue in `ui.rs`)
- [x] 1 Understand (confirmed 2026-11-10: "ja, stimmt so"; decision: only #408 rides along)
- [x] 2 Propose (pre-build questions answered, see Notes; approved 2026-11-10: "Go")
- [x] 3 Build (core 1543+18, GTK 171, Android 300 green; clippy GTK+core all-targets, CI lib -D warnings and Android target clean; lines 61,623)
- [x] 4 Review: round 1 (reviewer + `/code-review high`), no fix review (no behaviour change in the fixes); `/simplify` below
- [x] 5 Specs, docs, list (check-specs green; REFACTORING: #418 #353 #397 #505 #506 out, #468's Interval Bells rest kept with its reason, #505's Setup mark into #312, #397's `bell_sound_path` into #375, Box Breath names into #285, new deferred #508, P2 ticked; OPEN-ISSUES: #408 out, #365 names the "Missing" case)
- [x] 6 Verify and hand over (phone test 2026-11-10: "rest der tests scheinen auch zu passen"; laptop test skipped by the maintainer: "wird schon passen")
- [x] 7 Ship

## Review Focus
- A helper called while its page is closed (preset chooser, bell chooser, guided Manage) -> it refreshes only what is open or always shown; a closed chooser is refilled when it opens, as today.
- A change in one mode while Setup shows another (preset Override Undo) -> the helper takes the mode it is given; Setup's list and the widget follow the current mode as today.
- A lock taken inside a helper while the caller holds one -> every helper takes `&db` or locks once itself; a second `lock_db()` under a held guard deadlocks (no caller holds one; checked per call site).
- `put_rows` with a changed row count -> new model, as today; same count -> rows set in place (a Switch keeps its widget).
- The Local star on a row whose file vanished meanwhile -> `set_local_starred` on a gone id writes nothing; the list re-reads.
- A replayed batch that reclaims a name, and one that keeps it -> refresh only for the renamed row (existing reclaim tests).
- Linux: the edit page open, a value changed, back to the list -> the list shows the change (rebuilt on `connect_shown`); the Setup count follows each change.
- Linux: a Log filter change while a sync marked the Log -> one reload that reads both, the mark cleared.
- Old database, second device, another language -> no schema, payload or text change.

## 1. Core
- [x] 1.1 The reclaim learns from the recompute whether the name changed; drop the read-back query (#506). Files: `db/events.rs` and the four name-claiming recomputes. Proof: the existing reclaim tests stay green; the read-back `SELECT name` is gone.

## 2. Android
- [x] 2.1 #408 first: `populate_interval_bells` reads the library once and names each sound through core's `sound_name` and `resolved_name_text`. Files: `ui.rs`, `app.rs`. Proof: `interval_bell_rows_say_when_a_sound_is_missing` (source test: `ui.rs` cannot run on the host), Expected: FAIL then PASS.
- [x] 2.2 `bell_sounds_changed`: rename, delete and the pull use it; the pull's bell-chooser refill moves into `refresh_after_pull`. Proof: `label_and_sound_renames_reach_widget_and_bell_list`, `a_pull_follows_the_guided_file_and_refills_choosers` adjusted.
- [x] 2.3 `presets_changed(ui, mode)`: chooser (if open), Setup list and widget, each reading the label names once through one helper (not one shared lock, see Notes); every preset create, override, star, move, rename, delete and Undo uses it. Proof: source tests that pin the old calls adjusted; Android suite green.
- [x] 2.4 `guided_files_changed(ui)` (Manage list and Setup list, no selection follow): import, star, move, rename, delete, Undo use it; rename, delete and Undo still follow the selection themselves. Proof: as 2.3.
- [x] 2.5 Vibration editor Save calls `refresh_after_pattern_change`; label delete drops the hand patch of the Edit label. Proof: as 2.3.
- [x] 2.6 `refresh_bell_rows` under one lock; interval-editor ranges set once in `build_ui`. Proof: as 2.3.
- [x] 2.7 `put_rows` for the preset chooser and list, guided Manage and Starred, Local rows (Interval Bells keep a new model, see Notes); `local-star-tap(key, starred)` from the layout, no folder rescan. Files: `ui.rs`, `ui/main.slint`. Proof: Android suite green; manual on the phone.

## 3. GTK
- [x] 3.1 Interval Bells: rebuild on `connect_showing` (the push and every return, before the page is visible) instead of at push plus `connect_shown`; edits on the edit page no longer rebuild; the list page's own delete and a create without an edit page still rebuild at once; one `on_changed` per change, one library read per rebuild with core's `sound_name` (#353). Files: `bells.rs`. Proof: `bell_pages_reread_names_when_shown` and the GTK suite green.
- [x] 3.2 A filter change reloads the Log at once and clears the app's Log mark (no flag; the callers' follow-up `refresh()` goes); `stats_dirty`/`log_dirty` private (#505). Files: `log/mod.rs`, `log/imp.rs`, `application.rs`. Proof: GTK suite green; manual (filter change reloads the Log).

## 4. Docs
- [x] 4.1 REFACTORING.md: #418, #468, #353, #397, #505, #506 out, #505's Setup mark into #312, P2 ticked; OPEN-ISSUES.md: #408 out of HH.

## Notes
- Build, my calls:
  - #408's test went red first (library read per bell, empty name), then green.
  - One `bell_names_changed(ui, sound_chooser)` replaces `refresh_after_pattern_change` and the written-out sound refresh: open choosers (bell, pattern), Setup bell rows, Interval Bells, the editor's sound and pattern names. Used by sound rename/delete, pattern delete/Undo, the vibration editor's Save and the pull.
  - `presets_changed` calls the three refreshes (chooser only while open) instead of sharing one lock: threading `&db` through them and the widget's guided rows (which lock themselves) would add code for a few ms on a tap. The label map is one helper now, read once per list.
  - `put_rows` also for the label chooser and three Stats lists (insights, label totals, chart bars); the heatmap's nested models stay (P9 #456).
  - Label rename and delete: the hand patch of the Edit label went (`refresh_label_users` already does it).
  - #506: a bool from `claim_name` was not enough (a test caught it): a row whose winning event carries the suffixed name claims it unchanged. `claim_name` returns the name it gave, the named recomputes return `Option<String>`, the reclaim compares.
  - GTK bells: the rebuild no longer calls `on_changed`; create, toggle, edit and delete call it once.
- Pre-build questions (fresh agent, 2026-11-10), my calls:
  - Interval Bells stay on a new model: Material's Switch fires `checked-state-changed` for a change from code too and drops its binding once tapped, so rows set in place would write a pulled or Stopwatch-dimmed state back as a user toggle. Their put_rows part of #468 stays in REFACTORING with that reason (needs an echo guard).
  - GTK bells: `connect_showing` replaces the push-time build and `connect_shown` (one build, page never empty); list-page writes rebuild directly.
  - Log filter: no third flag; the setter reloads and clears the mark.
  - `guided_files_changed` does not follow the selection; the follow stays where it is today (the pull defers it during a session). The Local star keeps its own pair.
- Pure refactor except #408; existing tests pin the behaviour, source tests that name the old calls get adjusted.
- Review round 1, our reviewer (PASS / NEEDS FIXES):
  - R1.1 IMPORTANT, fixed: the sound-rename scenario's test no longer pinned what `bell_names_changed` refreshes; it now asserts the interval bells, Setup's bell rows and the editor's sound name.
  - R1.2 MINOR, rejected: the pull could call `presets_changed`. A guided pull is always also a setup pull (a guided file marks both), so `presets_changed` plus `guided_files_changed` would build the widget twice on every guided pull; the hand-written pair keeps it once.
  - 4.1 (docs) is still open at review time, as planned for step 5.
- Review round 1, `/code-review high`:
  - C1 rejected: the Local star trusts the row's state. The Local page is re-read each time it opens and covers Setup, so the only other star path (Setup's unstar) can't run while its rows are shown; local files don't sync.
  - C2 rejected here, linked: a failed library read names every bell "Missing" (Android and GTK, and `resolve_sound_name` everywhere already). That is EE #365's class (a failed read shows as an empty or wrong list), fixed there for every list.
  - C3 fixed: the pull clones the bell chooser's tick instead of holding its borrow through the whole refresh.
  - C4 fixed: the sound import refills through `repopulate_bell_chooser` (no inline category map).
  - C5 rejected: same as R1.2.
  - C6 fixed (docs): proposal and task 2.3 now say what was built (one label-map helper, not one shared lock).
  - C7 fixed: `refresh_bell_rows` reads the sound library once for both names. The four Box Breath phase names (`refresh_boxbreath_cues`) still read it per phase: left with P6 (#285, the phase list).
  - C8 fixed: GTK create rebuilds the list only when no edit page follows.
  - C9 rejected: Setup's count after each edit-page change is one count query; filtering which fields can change it adds code to save a query per spin step.
  - C10 fixed: the interval-editor ranges sit after the translation is selected.
- No fix review: the round-1 fixes change no behaviour, data or sync (a test assert, a borrow clone, the same refill through the existing helper, one library read for the same names, a skipped wasted rebuild, a moved constant block). Suites green after them: GTK 171, Android 300, Android target clippy clean.
- `/simplify` (four angles), applied:
  - Reuse: core `label_names_from_db` (new, test `label_names_are_keyed_by_uuid`) replaces the Android helper and the two GTK copies (`presets.rs`, `timer/imp.rs`).
  - Altitude: the bell chooser's category lives in one `thread_local` and its tick is read from its rows, as the pattern chooser's is; `bell_names_changed(ui)` takes no argument and the pull no extra parameter (C3's clone went with it).
  - Simplification: `rebuild_list`'s doc says when it runs; the label recompute lost its extra block; the source tests share one `ui_fn` helper.
- `/simplify`, skipped:
  - Reuse: `bell_volume::read` for the Setup volumes. Tried; `alarm_volume`'s test bans that call in `ui.rs` so no playback reads a global level; the display keeps its one-line parse.
  - Altitude: the pull's hand-written preset refresh: recorded as deferred #508 (take the widget out of `refresh_guided_files` first).
  - Efficiency: pattern changes also refresh the Interval Bells and the editor's sound name, and the bell count is read twice per `bell_names_changed`: a few reads per tap for one helper instead of two; the library reads per phase are #285.
  - Simplification: `recompute_for` returning the name for one caller (about a wash in size; kept).
- Suites after `/simplify`: core 1544+18, GTK 171, Android 300; clippy GTK+core all-targets, CI lib -D warnings and Android target clean; check-specs green. Lines: 61,577 (base 61,764, -187).

## Hand-over
- Built: GTK `meditate-gtk/builddir/src/meditate` 22:02:12; APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` 22:03:18 (debug).
- Phone test steps (general, maintainer asked for fewer):
  1. Use the app normally for a few minutes: presets (star, rename, delete with Undo), guided files and the local folder, bells and sounds. Every list and the widget show each change at once.
  2. Delete a sound an interval bell uses: that row says "Missing" (#408).
- Laptop test steps:
  1. Interval Bells: open, edit a bell, go back, create and delete one: the list and Setup's count are right.
  2. Log: use a filter; the list follows.
- Commit message (proposed):

      android, gtk: one refresh per change

      After a write, Android called the same refresh lists by hand at
      every call site and rebuilt each list as a new model; the Linux
      Interval Bells page rebuilt on every spin step; the Log filter
      used the app-wide data mark; replay read a name back to learn
      what it had just written. One helper per kind of change, rows
      updated in place, one library read per list, and on the phone an
      interval bell whose sound is gone now says "Missing" (#408).
