# Tasks: dark-session-colours

Base: eb4af163ea072cc8ebfe66914268b1a069f4fb99; suites on it: android 272 green (run on the same tree before the commit), core and gtk untouched since their green run at 0a5a72a

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-30)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (no doc names these colours)
- [x] 6 Verify and hand over (Janek tested the Timer screens on the FP5 2026-10-30; note field stays as it is, his call; Box Breath checked too)
- [ ] 7 Ship

## Review Focus
- Dark mode -> the dark scheme is what it shows today; scenario "Discard during a session" covers the light case, dark is unchanged by construction
- Accent picks and system colours -> they replace MaterialPalette.schemes, so the dark scheme follows them
- Other WrapButton uses (dialogs, pages) -> defaults keep today's roles
- Pressed states -> use the same passed colours
- The note field -> left out, see proposal

## 1. Dark roles on the session screen
- [x] 1.1 Guard test `the_session_screen_takes_the_dark_roles`: between "// ── Running screen" and "// ── Log page", and in EndButtonView, every `MaterialPalette.` use is `MaterialPalette.schemes.dark.`, and EndButtonView binds all three WrapButton colours to `schemes.dark.` (`primary`, `onPrimary`, `outline`). Files: meditate-android/src/app.rs. Proof: FAIL on the base (main.slint Box Breath lines use MaterialPalette.primary), then PASS
- [x] 1.2 WrapButton: `in property <color> primary`, `on-primary`, `outline-color`, defaulting to the MaterialPalette roles; EndButtonView passes `MaterialPalette.schemes.dark.primary`, `.onPrimary`, `.outline` (camelCase fields); Box Breath square uses `MaterialPalette.schemes.dark.primary`. Files: meditate-android/ui/main.slint. Proof: 1.1 passes, MANUAL scenarios on the FP5

## Notes
- Pre-build check (my call): the guard also requires EndButtonView to bind the three dark colours, else dropping them would stay green.
- Review 1: three MINOR. (1) held: the guard now also fails on a bare WrapButton on the session screen. (2) held: it uses the existing `component` helper. (3) the note field stays light in light mode, which was my call: asked Janek at the device test. The fixes are test-only, so no fix review.
- /simplify: the dark accent is held once on the session screen (`running-canvas.accent`) instead of five copies, and the guard loop reads line by line (simplification + reuse). Skipped: one bool on WrapButton instead of three colours (same size, a judgement call). Efficiency and altitude: clean. A mutation check (one accent back to MaterialPalette.primary) made the guard fail.

## Hand-over
- Verified: android suite 273/273, clippy CI form and Android target clean, check-specs green. APK meditate-android/android/app/build/outputs/apk/debug/app-debug.apk (2026-10-30 13:31), installed on the FP5 with `adb install -r`.
- Test steps (FP5, light mode):
  1. Start a Timer session: Pause is a light accent fill with dark text, Discard light text with a visible outline.
  2. Pause, then stop: Resume/Save/Discard look the same way. Question for Janek: the note field stays a light box on black. Keep it or make it dark too?
  3. Start a Box Breath session: square, progress and dot ring look as in dark mode.
  4. Dark mode, the same screens: unchanged.
- Commit message (proposed):
  android: Dark scheme colours on the black session screen

  The session screen is black in both themes, but its buttons and
  the Box Breath square took the light theme's colours there, so in
  light mode Discard was dark olive on black. They now always take
  the dark scheme's roles, which still follow the accent.

  Fixes #15
