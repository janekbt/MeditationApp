# Tasks: bells-that-ring

Base: e472dc1; suites on it: core 1546, Android 294, GTK 177 green (run 2026-11-12)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (no .slint change planned; ui.rs glue touched)
- [x] 1 Understand (said back and confirmed 2026-11-12; muted bells of every kind leave out, Janek 2026-11-12)
- [x] 2 Propose (approved 2026-11-12)
- [x] 3 Build (core 1549, Android 294, GTK 177 green; GTK+core clippy all-targets, CI clippy and the Android-target clippy clean)
- [x] 4 Review (two rounds, the second minor-only; /simplify done)
- [x] 5 Specs, docs, list (check-specs green; ARCHITECTURE effect section and file map; REFACTORING P5 row, #269 gone, #61-area Related line; OPEN-ISSUES #279, #280, #293 gone, HH row)
- [x] 6 Verify and hand over (installed 2026-11-12; Janek tested steps 1 to 5 incl. his Manage Bells case on both apps; the two suspend steps not run on a device, the session holds a wake lock; core tests cover them)
- [x] 7 Ship

## Review Focus
- Stopwatch: no end, so no fixed-from-end bell and every repeating bell counts; the count keeps today's stopwatch rule (existing test `interval_bells_count` asserts).
- A fixed bell and a repeating bell due on the same woken tick: each kind rings once (the repeating rule is unchanged).
- A muted bell inside Box Breath cues: cues have no counter; only their effect shape changes (#269).
- Old data: a bell with no time, or a Timer length missing from settings (`timer_session_secs_from_db` default 30 min).
- Sync changing the Timer length or bells while Setup shows: the existing refreshes after a pull re-read the count.

## 1. Core
- [x] 1.1 #269: one `Effect::Fire { channel, phase, cue }` built by one `fire(..)`; `FireRoute`/`fire_route` go; both apps' dispatch and tests follow. Proof: the existing effect tests, rewritten; suite green.
- [x] 1.2 `a_muted_starting_or_end_bell_is_not_counted`: the session records whether its starting and end bell rang. Expected: FAIL then PASS.
- [x] 1.3 `a_starting_bell_skipped_by_a_suspend_is_not_counted`. Expected: FAIL then PASS.
- [x] 1.4 `after_a_suspend_only_the_latest_missed_fixed_bell_rings`: `fire_due_bells` keeps the latest due fixed bell, marks earlier ones unheard. Expected: FAIL then PASS.
- [x] 1.5 `a_muted_bell_is_left_out`: muted bells and cues drop out when the session's settings are built, and from the count. Expected: FAIL then PASS.
- [x] 1.6 `the_bell_count_leaves_out_bells_that_cannot_ring`: `interval_bells_count` counts what can ring before the Timer's set end. Expected: FAIL then PASS.

## 2. Apps
- [x] 2.1 GTK and Android: a saved length or signal setting refreshes the Bells page count and the main Bells row (one summary refresh sets both). Proof: MANUAL scenario, on the device.

## 3. Docs and list
- [x] 3.1 REFACTORING.md P5 row and the four items; ARCHITECTURE if it names the effects; check-specs.

## Pre-build questions (fresh agent, 2026-11-12), my calls
- Fixed-bell rule after a jump (#279): no threshold; any tick past several due fixed bells rings the latest, ties all ring. Fixed bells are minutes apart in practice, and a late tick is under two seconds.
- "Can ring" for the count (#293) is the running counter's rule, one shared helper: a fixed bell before the length, a repeating bell whose earliest ring is before it; bells, not rings. An old repeating bell with no time still counts in the subtitle (it rings).
- The one effect (#269) carries the effective signal mode, as today's effects do; the four log tags stay, derived from channel and phase.
- The count reads the saved Timer length; the subtitle refreshes after the save and on the existing populate and reload paths.

## Build notes
- 1.1: the tests `fire_route_dispatches_each_fire_variant_to_its_channel`, `fire_route_returns_none_for_non_fire_effects` and `stopping_the_track_is_not_a_bell` went with `fire_route` (the type now says it); `each_fire_logs_under_its_own_tag` keeps the four log tags. `ActiveBell` carries a `BellCue`.
- Every new test failed first for the right reason (1.2 "1 of 3", 1.3 rung 2, 1.4 three bells, 1.5 count 2, 1.6 count 4).
- The muted rule lives in `Session::new` (drops muted starting, end and interval bells) and in `interval_bells_count`; `fire` still applies the mode's setting to what rings.
- Found, not mine: an unused `body` closure in android `app.rs::drop_files_take_one_path_and_report_failures` (test-only warning, there on the base).

## Review round 1 (reviewer + /code-review high, 2026-11-12)
Fixed:
- The main Bells row did not follow a saved Timer length (the length can change from the main screen): GTK `set_countdown_target` and Android `write_timer_session_secs` now also refresh the setup summaries.
- The main Bells row still named a muted starting bell, end bell or phase cues: `bells_summary` checks each against the mode's signal setting. Test `bells_summary_leaves_out_muted_bells`.
- Manage Bells showed a bell switched on that can't ring (Janek: grey it out as Stopwatch does). Test `a_bell_that_cannot_ring_is_greyed_off`.
- The preset subtitle counted by the old rule (Janek: same rule). Test `a_preset_counts_only_bells_that_can_ring`.
- One rule, `bells::RingLimits` (end + signal setting), now decides "can ring" for the count, the switches and the preset subtitle; it checks the first ring in O(1) instead of listing every ring. `is_bell_inert_in_stopwatch` and its test went (covered by `RingLimits`).
- The three tests above were written with their code in one step; each was checked by breaking its rule in the production code: all three failed, then the code was restored.
Rejected:
- No threshold for "only the latest fixed bell" (code-review): two fixed bells less than a tick gap apart (about 1 s) is not a real setup, the session holds a wake lock, and the rule is the approved one.
- "`rings_before` loops forever for a bell with no time" (code-review): it can't; `earliest_ring_secs` is at least 1 and `next_planned_secs` moves at least a second.
- "The mute rule lives in three places" (code-review): all three call `effective_signal_mode`, which is the one rule.
- "`Effect::Fire` can carry a phase on the Starting channel" (code-review): the approved #269 shape; one builder makes it, and the log tag has a test.
- Fold Android's other two `set_interval_bells_summary` lines into the new helper (code-review): both run with the database lock held; the helper would take it again.
- A bell with no time (old data) still counts in Setup while the running ring shows no beads for it, as before this change (my call, rare old data).

## Review round 2 (fix reviewer + /code-review high, 2026-11-12): minor only, so no third round
Fixed:
- `RingLimits::from_db` and `interval_bells_count` read the Timer's settings only (interval bells ring only in Timer); before, another mode's Stopwatch and signal setting could mix with the Timer's length.
- The preset subtitle skips a bell whose kind or signal doesn't read, as applying the preset does.
- Two more asserts: a muted bell in Manage Bells; muted phase cues leave the Bells row. `bells_summary`'s doc names all three reasons.
Rejected:
- The threshold for the fixed-bell rule, again (see round 1).
- A vibration-only bell chosen as "latest" on a device without haptics: the session doesn't know the device; the same holds for every vibration-only bell today.
- `bells_summary` reusing `RingLimits` for the cues: the cues follow the shown mode's signal setting, `RingLimits` the Timer's.
- Computing the count once on a length save (GTK and Android each read it twice): one list read per user action.
- A shared `ActiveBell` "fixed due" helper for the `due` closure: one caller.
Known limit (reviewer): a preset's subtitle counts its saved bell copies; applying it uses the library row by uuid, so a bell edited in Manage Bells after saving the preset can make the two differ. Fixing it needs the database in `preset_subtitle_parts`; not worth it now.

## /simplify (four angles, 2026-11-12)
Fixed:
- A starting bell skipped by a suspend now leaves the session's settings (`starting_bell = None`), as a muted one does; the `starting_skipped` field went. Test 1.3 still passes.
- One summary refresh per app also sets the Bells page count (GTK `refresh_setup_summaries` calls `refresh_interval_bells_count`, Android `refresh_setup_summaries` sets `interval_bells_summary`); a saved length or signal setting calls only that. Android's `refresh_interval_bells_summary` helper went. The main Bells row now also follows a signal change at once.
- GTK Manage Bells takes a plain `RingLimits` (read after the empty-list check); the shell's fallback copy of the switch rule went.
- Stale comments: `main.slint` (FireBoxBreathCue), GTK `dispatch_session_effects` doc.
- Clippy: `Option::is_none_or` is newer than the crate's MSRV; `map_or(true, ..)`.
Skipped:
- Drop `build_active_bells`'s own Stopwatch skip in favour of `RingLimits` (reuse): code outside this change; its `display` parameter would go with it.
- One shared `BellCue` literal for the five test helpers: tests only, each names its own sound.
- Efficiency: nothing to act on.

## Hand-over (step 6)
Checks: core 1551, Android 295, GTK 178 green; GTK+core clippy all-targets, CI clippy (-D warnings) and the aarch64 Android clippy clean; check-specs green.
Artifacts: `meditate-gtk/builddir/src/meditate` (19:38 after the device-test fixes, test app runs with `XDG_DATA_HOME=~/.cache/meditate-p5c-test`, log `~/.cache/meditate-p5c-test.log`); debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (19:39, installed). Build logs `~/.cache/p5c-gtk-build.log`, `~/.cache/p5c-apk-build.log`.
LOC (non-test .rs/.slint/.kt): 58,807 -> 58,761 (-46, after the device-test fixes); since the refactors began 62,729 -> 58,761 (-3,968).

Test steps (both apps unless named):
1. Bells as before: a Timer with starting bell, an interval bell and end bell, and a Box Breath session with phase cues: each rings and vibrates as it did.
2. Length: in Manage Bells, fixed bells at 5 and 30 minutes, switched on. Timer 10 minutes: the 30 minute bell shows off and greyed, the Bells page says "1 enabled", the main Bells row "1 interval bell". Set 40 minutes (on the main screen): both on, the counts say 2 at once.
3. Muted: set the 5 minute bell to Sound and the Timer's "what plays" to Vibration: that bell greys out and the counts drop at once. With a sound-only starting and end bell, the main Bells row doesn't name them; a session shows no bead and no count for them.
3b. With Manage Bells open, change the length from its start bar (10 to 12 minutes with a bell every 10 minutes): the bell is switchable at once.
4. Preset: save a 10 minute preset with bells at 5 and 15 minutes switched on: its subtitle says 1 bell.
5. Laptop, suspend: a 30 minute Timer with fixed bells at 2, 4 and 6 minutes; suspend at 1 minute, wake after 7: one bell rings, the 2 and 4 minute beads stay hollow, the counter reads "1 of 3".
6. Laptop, suspend through preparation: a 1 minute Timer with 30 s preparation, starting and end bell and a fixed bell at 30 s; suspend at 5 s, wake after 2 minutes: only the end bell rings, the counter reads "1".

Commit message:
```
core, gtk, android: bells that ring

The running counter heard a starting or end bell that never rang
(muted by the mode's signal setting, or skipped by a suspend through
preparation), a wake from a suspend rang every missed fixed bell at
once, and the Bells counts named bells that can't ring at the set
length. Core now drops a muted bell from the session, rings only the
latest missed fixed bell, and decides in one place whether a Timer
bell can ring; the Bells counts, the Bells row, the preset subtitle
and Manage Bells (greyed, as Stopwatch does) follow it. Inside, the
four bell effects are one.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```

## Device test fix 1 (2026-11-12)
- Found by Janek: changing the length with the start bar on Manage Bells (to 12 min, for a bell every 10 min) left that bell greyed; the list rebuilt only when shown. Fix: a saved Timer length also rebuilds an open Manage Bells, GTK through a weak handle to the page's rebuild (`TimerView::manage_bells`, `crate::WeakRebuilder`, `bells::push_bells_page` returns its `Rebuilder`), Android with `populate_interval_bells` while `interval-bells-page` is shown. Proof: the MANUAL scenario, extended; UI wiring, no host test.

## Review of device test fix 1 (round 3: fix reviewer + /code-review high, 2026-11-12)
Fixed:
- IMPORTANT: GTK's Manage Bells rebuilder holds itself (rows and its own closure keep strong clones), so the weak handle never died and a closed page was rebuilt on every length change. The page now drops its rebuild when it is popped (`connect_popped` on the nav view, filtered to this page, disconnects itself); that frees the rows and ends the handle.
- One place: the rebuild of an open Manage Bells moved into each app's summary refresh (GTK `refresh_setup_summaries`, Android `refresh_setup_summaries`), which every bell input already ends in (length, signal setting, preset apply and Undo, sync). Tests `the_summary_refresh_rebuilds_an_open_manage_bells` in GTK `timer/imp.rs` and Android `app.rs` (UI wiring can't run on the host, so they read the source, as their neighbours do); both failed first.
- The GTK rebuild runs with the handle cloned out of its RefCell borrow.
- Counted once: GTK `refresh_streak` no longer computes the count in its batched read (`refresh_setup_summaries` sets it); Android computes it inside the summary's one lock.
Rejected:
- Replace the weak handle with a visible-page tag check (layering): the window doesn't own the list's rebuild; the handle is the smallest link, and now dies with the page.
- A sync pull while Manage Bells is open (reviewer, minor): covered by the move to the summary refresh, which a pull ends in.

## Review round 4 (fix reviewer + /code-review high on round 3, 2026-11-12): minor only, no fifth round
Fixed:
- GTK: rebuilding Manage Bells from `refresh_setup_summaries` built it twice on open (the Bells page's `connect_hidden` runs that refresh under the freshly pushed page; #353 "one build when opened"). The rebuild now runs from the two places that change bell inputs while the list can be open: `set_countdown_target` (its start bar) and `refresh_streak` (sync, preset apply, Undo). Test renamed `an_open_manage_bells_follows_the_bell_inputs`, failed first.
- GTK: a second tap during the push built a second, orphaned page and pointed the handle at it; `push_bells_page` returns when the page is already there.
- GTK: the popped handler takes the rebuild with `try_borrow_mut`, so a pop during a rebuild can't panic.
- GTK: the row's delete button held its own row strongly (a cycle older than this change, now hit on each rebuild); it holds it weakly.
- Android test: the new test had taken the stopwatch test's doc comment; moved back.
Rejected:
- Rebuild the GTK list through weak handles instead of the popped handler (code-review): it means rewriting every rebuilder clone in the older list code; the popped handler ends the cycle in one place.
- `refresh_streak` keeps a stale count when the database can't be borrowed (code-review): then the whole Setup read fails too.
- Android computes the count twice while Manage Bells is open (summary and `populate_interval_bells`): one cheap query, and only with the page open.
- The Stopwatch switch doesn't rebuild an open Manage Bells: it lives on the Session page; Manage Bells rebuilds when shown.

## /simplify after the device-test fixes (four angles, 2026-11-12)
Fixed (GTK):
- `refresh_setup_summaries` reads the interval count inside its one database read and sets the subtitle there, instead of a second `refresh_interval_bells_count` borrow; that helper's stale doc now says it serves the bell-library page.
- `rebuild_open_manage_bells` flattened with let-else; `window::push_bells_page` lost its extra clone block.
Skipped:
- Refresh the GTK summaries only when the Bells page leaves the stack (so the rebuild could sit in the summary refresh, as on Android): it changes when the Bells and Session pages refresh, outside this fix; the two named call sites are pinned by a test.
- Android's three writers of `interval_bells_summary` (summary, reload, `populate_interval_bells`) and a second rebuild on a pull with the page open: older code with many callers; one cheap query.
- Rebuild the open list only when `RingLimits` changed: a save from the start bar is a rare user action.
- One weak `TimerView` instead of the toggle helper's three closures: older shape.
- Reuse: nothing found.
