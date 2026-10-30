# Tasks: pills and the lotus

Base: c2a80d4; suites on it: android 282 green, clippy clean (verified before shipping c2a80d4)

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`; `slint:slint` (loaded this session, no /compact since)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-30, lotus part shown, pills on the say-back; Janek: go)
- [x] 3 Build (283 green)
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (no doc names these)
- [x] 6 Verify and hand over (Janek: "Passt", 2026-10-30)
- [x] 7 Ship (specs merged, archived)

## Review Focus
- Notification shade expanded (large header icon) -> same small icon, covered by the device scenario.

- Streak text empty at start -> "Before the streak is known".
- Long label names, other languages -> "A long label name".
- Unlabelled card -> "Two labels in the Log".
- Dark mode, System accent -> "A streak with Rose and with System"; label colours already switch with the theme.
- Layout rule (only Material roles, no fixed colours) -> the existing test the_layout_uses_only_material_roles runs over the new code.

## 1. Lotus
- [x] 1.1 Test: notifyComplete's body uses `setSmallIcon(R.drawable.ic_launcher_foreground)` and no `android.R.`. Files: `src/app.rs`. Proof: `the_complete_notification_shows_the_lotus`, Expected: FAIL then PASS
- [x] 1.2 Change the small icon. Files: `kotlin/MeditateSessionService.kt`. Proof: 1.1 passes; device scenario

## 2. Streak pill
- [x] 2.1 Streak Text inside a Rectangle pill (primary at 15 % behind, primary text, bold, to-uppercase, letter spacing), hugging the text, hidden while the text is empty. Files: `ui/main.slint`. Proof: MANUAL scenarios; the_layout_uses_only_material_roles stays green

## 3. Label pill
- [x] 3.1 Card label as a pill: label colour at 14 % behind, 6 px dot, name in the label colour, wrapping, hugging. Files: `ui/main.slint`. Proof: MANUAL scenarios; the_layout_uses_only_material_roles stays green

## Notes
- Pre-build (my call): notifyComplete sits in the companion object (8-space indent), so the test cuts its body at "\n        }\n" itself instead of using kotlin_fn.
- The pills are layout-only and proven on the device (MANUAL), like the other appearance scenarios; a source test would only restate the layout.
- Pre-build (my call): the empty streak pill keeps its height and is only invisible (visible: false keeps its layout space), so the time readout doesn't jump when the streak arrives.
- Review 1 (fixed): "Label colours follow the accent" now names the chips and dots on both apps (MODIFIED); the label pill's corner radius is capped at 11 px so a wrapped name keeps its corners inside the tint. Streak pill left as a full stadium (one line in every language at 360 px).
- /simplify: applied: the lotus test checks the whole service file (both small icons are the lotus, no android.R. anywhere), padding shorthand in both pills, dropped a default alignment. Skipped: a shared pill component (two uses, differ in nearly every setting); dropping the dot's centring wrapper (unverified that Slint centres a fixed-height child; the same wrapper is used elsewhere).

## Hand-over
- Verified: android 283 green, clippy clean, check-specs green, debug APK 2026-10-30 20:45 installed on the FP5.
- Test steps (FP5): 1. Timer page: streak in an accent pill, capitals; try Rose and System, light and dark. 2. App start: no empty pill flashes, the time doesn't jump. 3. Log: labelled cards show a pill in the stripe's colour with a dot; unlabelled cards none; a long label name wraps inside the pill. 4. Let a timed session end with another app open: "Meditation complete" shows the lotus in the status bar.
- Commit message:
  android: Streak and label pills, lotus on "Meditation complete"

  The laptop shows the streak above the timer and each Log card's label
  in tinted pills; the phone showed plain text. Both are now pills like
  the laptop's: the streak in accent capitals on an accent tint, the
  label with a dot in its own colour on a tint of it. The "Meditation
  complete" notification shows the lotus in the status bar instead of
  Android's play triangle, like the running session.
- Device test 1 (Janek's screenshots): text sat high in both pills (measured: streak caps 1.5 px, label 3 px above the pill centre on the FP5); padding shifted 0.5 px / 1 px down.
