# Tasks: slider-holds-page

Base: dda64c5; suites on it: core 1390, android 261 green (Linux not
run: the change does not touch it).

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand (Janek's rule: touching the slider turns off page scrolling)
- [x] 2 Propose (approved: 2026-10-29, Janek "go")
- [x] 3 Build (core 1390, android 262 green; CI clippy clean; new test failed first on the missing global)
- [x] 4 Review, then `/simplify` after the last review (review; fixes were test and spec text only, so no fix review; /simplify applied)
- [x] 5 Specs, docs, list (check-specs green; no doc names the slider; #199 has no OPEN-ISSUES entry on beta)
- [x] 6 Verify and hand over (Janek, 2026-10-29, APK 13:16:09: "passt, alles funktioniert")
- [x] 7 Ship (2026-10-29, Janek "go")

## Review Focus
- Two fingers, one on the slider -> Slint sends the slider an up when the second finger lands: the value is saved and the page is free again; if one lifts and the other rests on a slider, that slider takes the touch. Accepted, no scenario.
- The page takes the touch before the slider gets it (vertical move over 8 px within 0.1 s) -> the page scrolls, the slider never got the touch and keeps its value; no scenario needed.
- A system cancel (shade, back gesture, app switch) -> arrives as an up: value saved, flag cleared, as today.
- The slider destroyed mid-press -> no one-finger path found (pages slide, dialogs keep the page); no safety net.
- Other Flickables (Log, Stats, Preferences) -> no volume slider there, unchanged.

## 1. Hold the page while a slider is touched
- [x] 1.1 Test first: `a_touched_volume_slider_holds_the_page_still` in `meditate-android/src/app.rs`, a source check (the Slint touch handover can't run on the host): `VolumeSlider` sets a global held flag true first in the down branch and false as the last line of the up and cancel branches (after the lines the old test matches); `interactive: !SliderTouch.held` appears exactly twice, on the setup Flickable and the interval bell editor's Flickable. Expected: FAIL then PASS.
- [x] 1.2 Code: a plain (not exported) `global SliderTouch { in-out property <bool> held; }` in `meditate-android/ui/main.slint`; `VolumeSlider`'s `pointer-event` sets and clears it; the two Flickables bind `interactive`.
- [x] 1.3 FP5: the manual scenario.

## Notes
- Builder check (fresh agent): the approach holds in Slint 1.18.1: with the slider's down delivered, every later move asks the Flickable's filter, which returns ForwardAndIgnore while `interactive` is false. After a held drag the Flickable keeps a stale press until the next touch; harmless. Answers above are my calls.

## Review (fresh agent)
- 1 IMPORTANT, fixed: the living requirement "A volume drag the page takes over changes nothing" said a swipe starting on the slider scrolls and puts the value back; now only a quick swipe in the first 0.1 s does. MODIFIED delta narrows it (wording only, same test).
- 2 MINOR, fixed: the test only checked `contains`; it now checks held first on down, freed last on up and cancel, and freed nowhere else. Proven by moving the free above `released`: the test failed, then restored.
- 3 MINOR, accepted: a slider destroyed mid-press would leave the page held. The only route found needs two fingers, and the second finger already makes Slint send the slider an up. Slint has no destroy callback to hook.
- Not checked by anyone: whether a GtkScale in a ScrolledWindow on the Librem 5 touch screen has the same problem; the proposal's "Android-only" rests on the report coming from the FP5.

## /simplify (four agents)
- Applied: the two slider tests share `volume_slider_source()` and `pointer_branch()`; the declaration check is gone (the compiler proves it); the branch checks trim whitespace; the page check is one `contains` per page. The order check still fails when the free moves above `released` (tried, restored).
- Skipped: binding `interactive` on every Flickable (altitude): harmless but touches eleven unrelated pages; the two pages are named in the spec.
- Skipped: a safety net for a slider destroyed mid-press: no one-finger route (review 3).
- Noted for later (altitude): the root cause is Slint's Flickable taking any drag that moves 8 px on its axis within 0.5 s, even a mostly sideways one; worth an upstream issue, not a vendored patch.

## Hand-over (step 6)
APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (12:34:40), debug, `adb install -r`.

FP5 steps:
1. Timer setup, a bell with its Volume row open: put a finger on the slider, drag left and right while moving clearly up and down, lift. The slider follows sideways, the page does not move, the value stays where you lifted (no snap back).
2. Swipe up and down beside the slider: the page scrolls as usual.
3. A quick flick upward that starts on the slider: the page scrolls, the slider keeps its value.
4. The same as 1 in the interval bell editor and in a Box Breath phase cue.

Commit message:
```
android: A touched volume slider holds the page still

A drag on a bell's volume slider that drifted up or down let the page
take over, and the slider snapped back. Now the page doesn't scroll
while a finger rests on a slider.
```

## Device test (step 6)
- Round 1 (Janek, 2026-10-29, APK 12:34:40): sliding holds the page, but on lifting the page scrolls afterwards, up or down. Cause (Slint 1.18.1 source, input.rs `handle_mouse_grab`, flickable.rs `handle_mouse`): while held the Flickable ignores every event but keeps the press position; the lift is handled in one pass that first gives the slider its up (which freed the page) and then sends a move at the lift point; the page, interactive again, read that move as one swipe from the landing point and scrolled by the whole vertical drift, then glided. Fix (fix 1): up and cancel start a 0 ms Slint Timer that frees the page, so it runs after that pass and before any next touch (Janek: no guessed delay; this is event-loop order). Test changed first and failed on it. Scenario THENs now say the page does not move when the finger lifts.
- Fix review of fix 1 (fresh agent): the cause and the 0 ms Timer hold (fires once on the next loop pass, `self.stop()` ends it). Two MINOR, both fixed test first: (a) a second tap read in the same input drain as the first lift could be freed by the first lift's pending timer; the down branch now stops a pending free (`free-page.stop()`); (b) the test now asserts that up saves the value first.
- /simplify on the fixes (four agents): applied a `ponytail:` note on the Timer (it works around Slint's Flickable keeping its press while not interactive) and computing the up branch once in the test. Skipped: moving "up saves the value" out of the test (it is this scenario's THEN, asked for by the fix review); whitespace-free order checks (house pattern of the slider tests).
