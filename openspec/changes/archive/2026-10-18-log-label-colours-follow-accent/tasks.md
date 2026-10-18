# Tasks: log label colours follow the accent

Base: f9c66ba1c257bd82006528348b323272b945111a; suites on it: green (per crate: core 1377+18, android --lib 222, gtk 150; 2026-10-18)

## Loop
- [x] 1 Understand (OPEN-ISSUES #186; decisions: colour wheel from the accent, order of creation, 16 colours)
- [x] 2 Propose (approved: 2026-10-18, "go")
- [x] 3 Build
- [x] 4 Review (one reviewer, R1-R4 below)
- [x] 5 Specs, docs, list (check-specs green; README, AGENTS, BUILDING; #186 off OPEN-ISSUES)
- [x] 6 Verify and hand over (device test passed 2026-10-18, after the stripe inset)
- [x] 7 Ship (delta merged, archived 2026-10-18)

## Review Focus
- Second device, offline creations -> "Two devices created labels before they synced".
- Seed labels ("Meditation", "Box-Breathing") seeded on each device with the same uuid: several created events for one uuid -> the earliest counts, so all devices agree after sync; before the first sync two devices may differ. Covered by the sync scenario's rule; no own scenario.
- A new device's seed events (low sync clock) can become the earliest for the two seed labels; if an existing device's seeds had a later clock than some user label, its colours reshuffle once after the new device syncs. Every device still agrees; accepted (my call).
- Label merged into another -> it is deleted: "A label is renamed or deleted".
- Name conflict "(conflict-...)" -> a separate label with its own event: its own colour, no scenario.
- Backup import that creates labels -> they emit created events like any new label; no scenario.
- A label with no created event (none expected) -> last, by local id; no scenario, noted in Decisions.
- Dark mode -> Android dark palette in "A green accent"; Linux: `@accent_color` and the text colour differ per mode.
- High contrast (Linux) -> follows `@accent_color`; MANUAL only if Janek uses it.
- Android accent 0 (shipped blue) -> colour 1 is the shipped primary.
- Many labels and sessions (2000+ cards) -> one query per Log load, not per card.
- Query fails -> follows the Log's existing load-error path (the label map is loaded in the same place).
- Another language -> no text changes.

## 1. Core
- [x] 1.1 `Database::label_color_places() -> Result<HashMap<i64, usize>>`: labels ranked by the earliest `label_insert` event for their uuid (lamport_ts, device_id), labels without one last by id; place = rank % 16. Files: `db/labels.rs`. Proof: `labels_take_colours_in_the_order_they_were_created`, `a_rename_keeps_a_label_colour_and_a_delete_moves_later_ones_up`, Expected FAIL then PASS
- [x] 1.2 Two databases exchange events (the existing replay test helper), both give the same places. Proof: `synced_devices_agree_on_label_colours`
- [x] 1.3 Remove `format::label_color_class_index` and its tests once both apps use 1.1. Proof: suite green, grep finds no caller

## 2. Android
- [x] 2.1 `theme.rs`: 16-colour palette from a mode's primary. Proof: `label_colours_start_at_the_accent_and_step_round_the_wheel` (hues read back from RGB within 3 degrees; brightness relations as in the scenario), Expected FAIL then PASS
- [x] 2.2 `ui.rs`: `apply_accent` sets the light and dark palettes (accent 0 too); the Log cards take `color-index` from 1.1 (-1 without a label). `main.slint`: card `label-color` reads the palette by `Palette.color-scheme`. Proof: MANUAL "The accent is changed while the Log is open"

## 3. GTK
- [x] 3.1 Test loads `data/style.css` into a CssProvider after `adw::init()` and fails on any parsing error; first with a broken colour to see it fail. Then 16 `@define-color log_cN` relative to `@accent_color` and rules for `.log-c0`..`.log-c15`. Files: `src/log/mod.rs`, `data/style.css`. Proof: `the_label_colours_load_relative_to_the_accent`
- [x] 3.2 `log/imp.rs`: load the place map with the label map; `label_color_class` takes a place. Proof: MANUAL "The accent is changed while the Log is open" (cards show the colours by creation order)

## 4. Docs
- [x] 4.1 README accent line names the Log; OPEN-ISSUES drops #186; comments in `style.css` and `log/imp.rs`. Proof: grep

## Notes
- The sync test can only check that both devices agree: two fresh databases tie on the clock and break on random device ids.
- The places query starts from `labels` and joins events by target id (indexed), never scans events by kind.
- GTK reloads the place map wherever `self.labels` is reloaded (`refresh`, new-session path, `refresh_filter_labels`).
- `meditate-android/src/app.rs` source-scan test reads only the first 2500 bytes of `group_log_sessions`; widen it if the new parameter pushes its target out.
- Remove the `label_color_class_index` comment in `main.slint` (~234).
- The GTK test fails hard without a display, and is the only test in that binary that initialises GTK (my call).
- GTK 4.18 parses `oklch(from @accent_color l c calc(h + 45))` and `oklch(from color-mix(in oklch, @accent_color 65%, @window_fg_color) l c calc(h + 22.5))` in `@define-color` (probe 2026-10-18; a typo gives "Expected a number"), so the Linux colours need no Rust.
- The GTK test only proves the stylesheet parses; that `@accent_color` resolves at draw time is the MANUAL check (my call, a realised-widget test is much more code).
- The GTK test needs a display (gtk/adw init), like running the app; CI doesn't run GTK tests (my call).
- The local event log is never pruned (sync compaction only merges remote batch files), so the created events stay available.
- Built: GTK second ring mixes in oklab, not oklch (mixing in oklch with the hue-less text colour could swing the hue), and ends in "/ 1" because libadwaita's text colour is 80 % opaque. Resolved colours probed in light and dark (blue accent): ring 1 blue, purple, magenta, red, orange-brown, olive, green, teal; ring 2 the darker (light mode) or paler (dark mode) set, all distinct.
- 2.2 and 3.2 are code-complete; their proof is the MANUAL device check.
- Android-target clippy clean; host clippy (both lines in AGENTS.md) clean.

## Review (2026-10-18, one reviewer)
- R1 [IMPORTANT] a device that creates labels or restores a backup before its first sync ranks them early and moves the others once: taken to Janek; he chose to keep it and write it down. Scenario "A device made a label before its first sync", test `a_label_made_before_a_device_first_synced_moves_the_others_once` (documents behaviour that existed: passed at once; ordering by local id instead makes it fail).
- R2 [MINOR] a failed places query was silent: now logged to the diagnostics log in both apps. The Log's other loads (labels, names, pages) are silent the same way, older than this change.
- R3 [MINOR] stylesheet test didn't check the stripe, chip and dot rules: it now does for all 16 (deleting `.log-label-dot.log-c12` makes it fail). AGENTS.md and BUILDING.md say the GTK suite needs a display.
- R4 [MINOR] Android test builds from `accent_fields`, not through `apply_accent`: set aside, three lines of glue covered by the MANUAL check.

## Hand-over (2026-10-18)
Suites after the fixes: core 1378+18, android --lib 223, gtk 151, all green; clippy (both AGENTS lines and the Android target) clean.
Artifacts: `meditate-gtk/builddir/src/meditate` (09:54, run with `XDG_DATA_HOME=<scratchpad>/gtkdata2`), `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (09:56, installed on the FP5).

Test steps:
1. Linux: create a few labels (Settings or the label picker), add sessions with them (Log, Add). The stripe, chip and dot follow the accent; the first label is the accent colour, the next ones step round the wheel; colours 9+ darker.
2. Linux: GNOME Settings, Appearance, change the accent with the Log open: the colours change at once. Same with dark style.
3. FP5: Log with labelled sessions; Preferences, change the accent: stripe and label name take the new colours; dark mode too. Unlabelled sessions stay grey.
4. Both: rename a label, it keeps its colour; delete an earlier one, the later ones move up one.

Proposed commit message:
    log: Label colours follow the accent and creation order

    The Log coloured labels from a fixed palette chosen by a hash of the
    name: it ignored the accent, and two labels often shared a colour.
    Now sixteen colours come from the accent, and labels take them in
    the order they were created, the same on every synced device.

    OPEN-ISSUES #186.
- Device test 2026-10-18: all fine; one note: the Android stripe sat on the card's left edge, GTK's is inset 6 px. Android card row now has 6 px left padding (layout only, no second review).
