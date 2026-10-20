# Tasks: Material 3 colour roles on Android

Base: b136988 (beta); suites on it: green (`cargo test -p meditate-android`: 250 passed). Core untouched by this change, its suite not run on the base.

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`, `slint:slint` (Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-20)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (check-specs green; README widget line, ARCHITECTURE Colours; no OPEN-ISSUES entry)
- [x] 6 Verify and hand over (FP5 tested and approved 2026-10-20)
- [x] 7 Ship (repro probe 23/23 identical; specs merged, archived)

## Review Focus
- A stored accent from the previous release ("+3", "3", "system"): values and parsing are unchanged, only the colours behind an index change. No scenario; the existing accent tests keep covering it.
- Another device on the previous release shows Rose in its old HSL look. Accepted: only the look differs, the pick is the same.
- Below Android 14 without System: the fixed accents are the only colourful choice, covered by "Picking Rose".
- Label colours under a fixed accent: still derived from its primary, which now comes from the tonal-spot scheme. Covered by MANUAL "Rose on screen" (Log).
- The Material library's date/time pickers keep a 50 % backdrop next to our 32 %. Accepted, `material-1.0/` is build environment.
- Widget below Android 12: resource qualifiers fall back to `values/`. Covered by MANUAL "The widget below Android 12".
- Widget on Android 12/13: the tonal-palette resources exist from API 31. Covered by MANUAL on the FP5 (15) and the reason that the names are API 31 resources.
- F-Droid: one new crate from crates.io, vendored by F-Droid's build like the others. The repro probe runs on the final tree.
- Startup cost: the picked accent builds one light+dark scheme; the five circles each build only a light primary. No scenario.

## 1. Tonal-spot accents (theme.rs, Cargo.toml)
- [x] 1.1 Add `material-colors = "0.4.2"` to `meditate-android/Cargo.toml`; check that Cargo.lock gains only that crate. Proof: `git diff Cargo.lock` shows one new package.
- [x] 1.2 Tests first: `a_seed_gives_materials_tonal_spot_scheme` (all roles Material's 2023 Swift test gives for seed #FA2BEC, light and dark, hex written by hand), `a_fixed_accent_takes_every_role_from_its_seed`, `blue_stays_the_shipped_scheme`, `a_circle_shows_its_accents_light_primary`. Expected: FAIL (stubs), then PASS.
- [x] 1.3 Code: `ACCENTS` holds seed colours (today's swatches); a fixed accent's light and dark schemes are tonal spot from its seed for every role; the swatch is the light primary. Remove the HSL accent families and their tests, keep `hsl`/`to_hsl` only where `label_palette` needs them. Proof: the four tests pass, `cargo test -p meditate-android` passes.

## 2. Roles in the layout (main.slint, app.rs)
- [x] 2.1 Test first: `the_layout_uses_only_material_roles` reads `ui/main.slint` and fails on `.darker(`, `.brighter(`, `.transparentize(` and any hex colour outside the running screen and the System wand. Expected: FAIL now (they exist), PASS after 2.2.
- [x] 2.2 main.slint:
  - ChooserRow, ExpanderRow body, the third child-shade row: `surface-container-low`, hover `surface-container-low.mix(on-surface, 0.92)`, pressed `.mix(on-surface, 0.90)`.
  - CompactToggle, the equal-width toggle, preset rows: selected text `on-secondary-container`.
  - WrapButton: filled pressed `primary.mix(on-primary, 0.90)`; outlined pressed `primary.with-alpha(0.10)`.
  - Play/Stop pill: text `primary`, background pressed `primary.with-alpha(0.10)`, hover `primary.with-alpha(0.08)`.
  - All `#00000080` scrims: `MaterialPalette.scrim.with-alpha(0.32)`; snackbar shadow `MaterialPalette.shadow.with-alpha(0.25)`.
  Proof: 2.1 passes; MANUAL screenshots per the appearance delta.

## 3. Widget (res/)
- [x] 3.1 `values-v31/colors.xml` and `values-night-v31/colors.xml` override `widget_bg`, `widget_fg`, `widget_fg_muted`, `widget_row_bg` with the system tonal colours in the proposal. Update the comment in `values/colors.xml`. Proof: MANUAL on the FP5 and the API 30 emulator.

## Notes
- Pre-build questions (fresh agent), answered:
  - Crate API: `SchemeTonalSpot::new(hct, dark, None)` (the 2023 dynamic scheme), not `Scheme::light` (2021). The reference test must match exactly, with no tolerance. If it doesn't, stop and ask before going on (my call).
  - Test 1.2 goes through our own crate→`MaterialScheme` mapping. The roles the Swift test lacks (surface containers, dim/bright, tint, fixed) are checked against the crate accessor of the same name, so a swapped field fails. `a_fixed_accent_...` is a wiring check (Rose ≠ baseline and = tonal spot of Rose's seed). Accepted as such (my call).
  - Comments that mention `.darker(` are rewritten, so the guard matches all text (my call).
  - Seed hex: compute today's swatches from the old HSL code before deleting it, and write them down by hand.
  - cargo-sources.json is regenerated at release per RELEASE.md. Until then a manual Flatpak CI run fails; PR CI doesn't run on direct pushes (my call, said to the maintainer).
  - The widget becomes opaque: the system colours have no alpha, and Material widgets are opaque (my call, said to the maintainer).
  - Child rows: hover 8 % (`mix(on-surface, 0.92)`), pressed 10 % (`mix(on-surface, 0.90)`) (my call).
- Build:
  - Seeds: the old `swatch()` was checked against the hex values with a throwaway test before deletion: Green 328C50, Teal 328C8C, Purple 66328C, Rose 8C3241, Amber 8C6E32.
  - Cargo.lock gained exactly one package (material-colors 0.4.2).
  - `blue_stays_the_shipped_scheme` passed at once: the behaviour existed and the plan expected that (Blue stays verbatim).
  - The crate matches Google's 2023 reference exactly; there was no tolerance needed.
  - Removed the HSL-family tests `accent_light_and_dark_use_m3_tone_relationship` and `on_colors_contrast_with_their_base` (their code is gone). The label-palette test now feeds a fixed HSL colour instead of the removed `accent_fields`.
  - Two older source tests matched the old text: `the_active_preset_is_highlighted` (selected name colour) and `a_back_swipe_does_not_tap_a_backdrop` (scrim literal). Both were updated to the new roles.
  - The selected preset's subtitle also takes on-secondary-container, because on-surface-variant on the secondary container is the same mismatch (my call).
  - Suite: 253 passed. CI clippy (-D warnings) and Android-target clippy are clean.
- Review (fresh agent), all findings checked and fixed:
  - Guard too narrow (IMPORTANT): it now rejects every hex colour outside the running screen and the System wand, plus `.transparentize(`. Mutation check: `#00000066` in a dialog fails it.
  - Stale `apply_accent` doc in ui.rs: reworded.
  - `swatch` built a full light+dark scheme per circle: it now builds only the light primary. Same value, still covered by `a_circle_shows_its_accents_light_primary`.
  - Stale widget drawable comments: reworded for below 12 / 12+. The "white lotus" comment is accurate, because the icon is the full app icon on its blue square and stays visible on a light widget.
  - ARCHITECTURE.md "Colours": rewritten.
  - Task 3.1 ticked. The scenario wording "darkens" was wrong in dark mode and now reads "shifts toward the text colour".
  - These fixes change no behaviour, data or sync, so no second review is needed.
- /simplify (4 agents), applied:
  - State layers use `MaterialPalette.state-layer-opacity-hover/press` instead of typed 0.08/0.10.
  - `swatch` is back to `tonal_spot(seed).light.primary`: one conversion path. The efficiency agent measured the cost as negligible, which overrides the earlier review's micro-optimisation.
  - One `use material_colors` at the module top and one `argb()` conversion.
  - The guard also bans `rgb(`, `rgba(`, `hsv(` and `Colors.`.
- /simplify skipped:
  - A shared `Scrim` component for the 8 scrims: the duplication predates this change, and an existing test counts the copies.
  - The library `StateLayer` component: the rows would need restructuring, and it depends on the library's private window adapter.
  - Replacing the guard's comment anchors with a colour global: the anchors fail loudly if renamed, and that is good enough.
  - Pairwise checks instead of the crate comparison in the reference test: they wouldn't catch a swap of two distinct values.
  - `hue_and_lightness` vs `to_hsl` in the tests: predates this change.
- Emulator (API 35, MCP debug APK):
  - Found that the Timer page showed Slint's fixed #fafafa between the cards, because MainWindow had no background.
  - Added the scenario "The page behind the cards" with the test `the_window_background_is_the_surface_role` (FAIL, then PASS) and the fix `background: MaterialPalette.surface;`.
  - Rose checked in light and dark: tinted surfaces, child rows on container-low, the 32 % scrim behind the duration dialog, and the selected segments readable.
- Fix review (fresh agent, post-review fixes only), applied:
  - The guard also catches CSS colour names (white, black, ...) and `oklch(`/`hsl(`. Mutation check: `color: white;` fails it.
  - The requirement text now names the System wand as exempt.
  - The agent confirmed `background: MaterialPalette.surface;` shows only where nothing painted before (the Timer setup page, slide gaps), and that the running screen is unchanged.
- /simplify again (on the fixes), applied:
  - `swatch(baseline, accent, system)` is `accent_schemes(..).light.primary`, so the System circle goes through it too. The match in ui.rs is gone, and the index-0 rule lives only in `accent_schemes`.
  - One combined fixed-colour assert.
  - Both new tests use `slint_code()`.
- /simplify again, skipped:
  - Wand coloured with on-primary: it would follow the current accent, not the System circle's colour.
  - A looser window-background match: the exact line is fine.
  - swatch building full schemes: negligible, and one path is worth more.
- Suite 254 passed. CI clippy and Android-target clippy are clean.

## Hand-over
- APK (debug, phone): `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (built 2026-10-20 19:38).
- Repro probe: not run yet. It runs on the final tree after the device test and before the push, because Cargo.toml and Cargo.lock changed.
- Test steps on the FP5:
  1. Preferences > Appearance: the circles show System, Blue, then the five colours. Pick Rose: app bar, cards, the page between the cards and the navigation bar all turn faintly rose, in light and in dark mode. Text is readable everywhere.
  2. Timer: turn on End Bell. The rows under it (Type, Sound, Volume) sit one shade off the card: lighter in light mode, darker in dark mode. Press and hold Sound: the row shifts slightly toward the text colour.
  3. Selected segments (Timer/Guided/Box Breath, Signals, Type) and a selected preset: the text has the dark (light mode) or light (dark mode) on-colour of the fill, and is readable.
  4. Press and hold "Speichern" (filled) and "Verwalten" (outlined): a light state layer, not a darker button.
  5. In End Bell > Volume, the Play pill: text in the accent colour, and a faint accent fill while pressed.
  6. Open Duration (a dialog) and the Log filter: the screen behind dims less than before (32 %).
  7. Log with labels under Rose: the label colours are still distinct from each other.
  8. Widget on the home screen: background, text and rows in the system colours. Switch dark mode, and the widget follows. Set another wallpaper colour, and the widget follows.
  9. Back to System: everything as before this change, apart from points 2 to 6.
- Proposed commit message:

      android: Use Material 3 colour roles throughout

      Green, Teal, Purple, Rose and Amber are now Material's tonal-spot
      scheme from their seed (crate material-colors 0.4.2), surfaces
      included, instead of an HSL approximation over blue neutrals.
      Child rows, text on selected fills, pressed states, scrims, the
      snackbar shadow and the window background use Material roles; a
      test keeps fixed colours out of the layout. The home-screen widget
      takes the system colours on Android 12 and later.
