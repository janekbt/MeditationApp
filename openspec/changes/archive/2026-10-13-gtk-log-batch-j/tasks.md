# Tasks: GTK Log (batch J)

Base: d9e37ce6c9dbba7222c69fd3a523591a1173c4ff; suites on it: green (`cargo test --workspace`, 1703 passed)

## Loop
- [x] 1 Understand (all six issues re-checked in the code, all still present; Android shares #117)
- [x] 2 Propose (approved: 2026-10-13)
- [x] 3 Build (1712 passed; clippy clean on host and Android target)
- [x] 4 Review (two fresh reviews: the change, then fixes R1-R3; R4 from the second)
- [x] 5 Specs, docs, list (check-specs green; OPEN-ISSUES: batch J removed and ticked, idea #190 added; no other doc names the changed code)
- [ ] 6 Verify and hand over (built; waiting for the device test)
- [ ] 7 Ship

## Review Focus
- Window minimised or app in the background at midnight -> GTK's poll asks the day watch before its unmapped skip, so the reload runs even while hidden (and within 2 s of a wake-up).
- Clock set backwards (time zone change, manual) -> "a different day" counts as new, either direction (day watch scenario).
- A session running at midnight -> the reload only rebuilds Log and Stats, not the Timer page; the Log's pending deletes survive it (#123).
- New day during a pending delete -> covered by #123's rebuild scenario.
- Filter popover opened while the filter is set -> rebuilding the dropdown selects the same entry; the handler sees no change and neither reloads nor closes the popover.
- Two labels with the same name -> the dropdown maps by position into the ids it was built from, so the right one is picked.
- Edit that changes the minute -> seconds are kept (same as Android, which keeps them on every edit). (my call: matches Android)
- DST day edit -> `local_naive_to_unix` picks the first of a doubled time and moves a gap time forward, as Android.
- A delete of a card not on screen (filtered out by a rebuild) -> commit still deletes the row; caption and count stay as they are.
- Another language, an old database, a second device -> no text, schema or sync change.

## 1. #117 New day (core, GTK, Android)
- [x] 1.1 Core test `a_new_day_is_reported_once` in `meditate-core/src/time.rs`: a `DayWatch` started on 2026-10-12 reports false for 2026-10-12, true for 2026-10-13, then false for 2026-10-13, true for 2026-10-12 (clock set back). Expected: FAIL (no type), then PASS.
- [x] 1.2 GTK source test `the_window_poll_reloads_both_pages_on_a_new_day` in `window/imp.rs`: the 2 s poll asks the day watch before its mapped check is passed, invalidates LOG and STATS, and refreshes the visible Log or Stats. Expected: FAIL, then PASS.
- [x] 1.3 Android source test `the_tick_loop_reloads_both_pages_on_a_new_day` in `app.rs`: the tick loop asks a day watch and on a new day calls `refresh_stats` and `reset_log_feed`. Expected: FAIL, then PASS.

## 2. #118 Save under a filter
- [x] 2.1 Source test `a_save_under_a_filter_reloads_instead_of_adding` in `log/mod.rs`: `prepend_session` returns after `invalidate(...LOG)` when a filter is set, before the imp call. Expected: FAIL, then PASS.

## 3. #119 Label filter
- [x] 3.1 Source test `the_filter_shows_and_maps_the_labels_it_was_built_from`: `refresh_filter_labels` stores the shown ids and selects the active one; the window handler maps through the stored ids and returns when the id equals the active filter; the popover rebuilds the dropdown when shown. Expected: FAIL, then PASS.
- [x] 3.2 Source test `a_deleted_filter_label_clears_the_filter`: when the active id is not among the labels, `refresh_filter_labels` clears the filter and reloads. Expected: FAIL, then PASS.

## 4. #120 Start seconds
- [x] 4.1 Source test `an_edit_keeps_the_start_seconds`: the Save handler builds the start with `meditate_core::time::local_naive_to_unix` and the original start's second (0 for Add); no `glib::DateTime::new(` with `0.0`. Expected: FAIL, then PASS.

## 5. #121 Caption after delete
- [x] 5.1 Source test `a_delete_subtracts_what_the_card_added`: `commit_delete_in_place` subtracts `log_card_total_secs(session.duration_secs)`. Expected: FAIL, then PASS.

## 6. #123 Pending delete and rebuild
- [x] 6.1 Source test `a_pending_delete_stays_hidden_and_counts_once`: `commit_delete_in_place` returns unless `cards_by_id.borrow_mut().remove(&session_id)` found the card, before touching counts; `append_session_to_feed` and `prepend_session` hide a card whose id is in `pending_deletes`. Expected: FAIL, then PASS.

## Hand-over
- Suites: 1713 passed, 0 failed; clippy clean (host, CI form, Android target); check-specs green.
- Laptop: Flatpak installed 2026-10-13 16:16. Librem 5: binary built 16:20, copied, checksum 67a1de9e14585119 matches; DB kept (no schema change). FP5: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` built 16:18, installed 2026-10-13 18:12.
- Laptop: all steps passed (Janek, 2026-10-13). Librem 5 not tested: Janek decided the laptop test suffices, same GTK build and no device-specific feature.
- Test steps (both GTK devices unless noted):
  1. New day: Stats on screen, clock set by hand to 23:59, wait past midnight -> within 2 s today reads 0 and the heatmap's today moves; the Log heads the old day "Yesterday". FP5: Stats open, app in background over midnight, then back -> new day at once; a Log scrolled to page 3 keeps its place. Clock back to automatic; save no session while it is wrong.
  2. #118: Log filtered to label A, save a session with label B on the Timer, back to the Log -> B not shown; filter off -> B shown.
  3. #119: filter to A, change tab and back, open the filter -> shows A; pick "All labels" -> all sessions. Rename A, open the filter -> new name, filters right. Delete A while filtered -> filter gone, all sessions.
  4. #121: a day with a 10:20 and a 5:00 session, delete the 10:20 one and let the toast run out -> caption "5m".
  5. #123: delete a session, within 5 s edit another and save -> the deleted card stays away, toast "1 session deleted"; Undo brings it back; without Undo the caption is right.
  6. #120 has no visible seconds in the UI; covered by its test.
- Proposed commit message:

      gtk: Log rows and totals right, a new day noticed (batch J)

      Log and Stats notice a new day on their own in both apps, a
      filtered Log reloads instead of adding a non-matching card, the
      label filter shows and maps the label it filters by, an edit
      keeps the start seconds, a delete subtracts the minutes its card
      showed, and a card waiting for Undo stays hidden through a
      rebuild.

## Review fixes (first review)
- R1 A sync or merge that deletes the filtered label: `LogView::refresh` now clears a filter whose label is gone, before loading the page (the Log is on screen in that case, the dropdown is not). Proof: `a_deleted_filter_label_clears_the_filter`, extended.
- R2 The dropdown position is a pure function `filter_position` with its own test `the_dropdown_selects_the_active_label` (the source test could not catch an off-by-one).
- R3 Android at a new day calls `render_log_feed` instead of `reset_log_feed`: only the headers changed, so the loaded pages and the scroll stay. Proof: `the_tick_loop_reloads_both_pages_on_a_new_day`, changed.
- Rejected: "a note-only edit of a session in the doubled DST hour moves its start by an hour". Sessions store the start as local wall-clock text, and reading it back always gives the first of the two times; the dialog rebuilds that same wall-clock time, so the stored text is unchanged and nothing syncs. A core helper for it was written, then removed.
- R4 (second review) A failed labels read in `LogView::refresh` cleared the filter as if the label were deleted; the check now runs only on a successful read. Proof: `a_deleted_filter_label_clears_the_filter`, extended.
- Not changed (second review): the filter popover open while a sync deletes its label keeps showing the old row until it is opened again; picking it shows "no matching sessions" once, the next reload clears it. Rare and self-healing.
- Not changed (second review): the Timer page's streak line is not refreshed at midnight; left out in the proposal, and `refresh_streak` also reloads Setup values, so calling it mid-session needs its own look.
- Not changed: after a label merge the filter falls back to "All labels" rather than the surviving label (the spec says the filter clears; merge-following would be new behaviour).
- Not changed: GTK's visible Log rebuild at midnight resets its scroll (once a day, through the shared refresh).

## Notes
- The GTK tests read source text: the Log and window need a display, which the test host lacks.
- `stats_inputs_and_day_caption_come_from_core` in `application.rs` counted four uses of the rounded card minutes in `log/imp.rs`; the #121 delete is the fifth, so its count moved to 5.
- The #120 test first also caught the date row's `glib::DateTime::new(..., 0.0)` (a subtitle, not the start); it now looks only at the start's expression.
- The filter popover rebuilds its dropdown on show (my call, see proposal); a rebuild that clears a deleted label's filter reloads the Log while the popover opens.
