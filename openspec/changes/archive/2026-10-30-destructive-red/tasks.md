# Tasks: destructive-red

Base: d181c02b57b2dafc540ee6e1d3f38c1855a26f37; suites on it: android 273 green (same tree before the commit)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-30)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (no doc names these colours)
- [x] 6 Verify and hand over (Janek tested on the FP5 2026-10-30: "Works")
- [ ] 7 Ship

## Review Focus
- Light and dark mode in dialogs -> MaterialPalette.error follows the theme; on the black session screen the dark scheme's error
- A disabled confirm button (name dialogs) -> those are not destructive; the local button keeps the TextButton's disabled look anyway
- Accent and system colours -> error comes from the same schemes, so it follows the system's error tone
- Pressed state -> the state layer takes the button's colour, as in TextButton

## 1. Red destructive buttons
- [x] 1.1 Guard `destructive_buttons_are_red`: every `ConfirmDialog {` whose confirm-text is Delete, Delete All, Discard or Wipe Local Data sets `destructive: true;` (matching `ConfirmDialog {` uses only, not the component itself); the recovery dialog's "Wipe Local" is a destructive ConfirmButton; ConfirmDialog's confirm button picks `MaterialPalette.error` when destructive; EndButtonView sets `destructive` for kind 4 and WrapButton picks the error role (dark scheme when dark). Files: meditate-android/src/app.rs. Proof: FAIL on the base, then PASS
- [x] 1.2 main.slint: WrapButton `destructive`; EndButtonView `destructive: root.kind == 4`; a local `ConfirmButton` on BaseButton (text, enabled, clicked, accessibility, disabled look like TextButton) with `color: destructive ? error : primary`; ConfirmDialog `in property <bool> destructive` used for both confirm buttons; the eight dialogs set it; the recovery dialog's Wipe Local uses `ConfirmButton { destructive: true; }`; WrapButton's pressed tint follows the red too. Files: meditate-android/ui/main.slint. Proof: 1.1, MANUAL scenarios on the FP5

## Notes
- Pre-build check: the two wipe answers were added (GTK marks them destructive). The Delete-all-sessions preference row stays out, since it is a row. Android's guided delete has no dialog. BaseButton imports by path, like date_picker.
- Review 1: the IMPORTANT finding held. The living scenario still said "light accent text" for Discard, so a MODIFIED block now rewrites it to red. MINOR 2 held: the guard now requires the error colour on both the text and the outline. MINOR 3 is accepted as written: the guard knows the four destructive button texts the app has; a new wording would need adding there.
- /simplify: the Wipe Local check is now one literal `contains`. Skipped: dropping the exact `error` expression check, because the dark-roles guard alone would let `error` point at the dark primary. Also skipped: a looser test (altitude), because review 1 asked for this strictness. ConfirmButton as a TextButton copy was judged clean by all four angles. Efficiency and reuse: clean.

## Hand-over
- Verified: android suite 274/274, clippy CI form and Android target clean, check-specs green. APK meditate-android/android/app/build/outputs/apk/debug/app-debug.apk (2026-10-30 14:31). Installed on the FP5 with `adb install -r` after reconnecting.
- Test steps (FP5, light and dark mode):
  1. Start a session: Discard is an outlined pill with light red text and a red outline; tap-and-hold shows a red tint; Pause stays accent.
  2. Open a delete dialog (e.g. delete a label or preset), then cancel: Delete is red, Cancel accent.
  3. Pause a session with a note, tap Discard: Discard in the dialog is red.
- Commit message (proposed):
  android: Show actions that throw data away in red

  Discard on the session screen and the Delete, Discard and Wipe
  buttons of the confirm dialogs now use Material's error colour,
  as the Linux app's destructive buttons do. Discard stays an
  outlined pill so it doesn't pull the eye during a session.
