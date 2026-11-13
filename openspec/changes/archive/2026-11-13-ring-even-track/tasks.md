# Tasks: ring-even-track

Base: 9237d15 (core, gtk, android: the ring is built only when it changes); suites on it 2026-11-13: core 1554 green, GTK 181 green, Android 296 green

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand (cause said back; Janek: "Ja, bitte auf beiden Plattformen fixen")
- [x] 2 Propose (pre-build questions answered, my calls: the test compares the pixels exactly and moves the probe if antialiasing alone differs; `accent_rgb` stays as it is; Janek then decided the Android centres become black) (approved 2026-11-13, Janek: "Go")
- [x] 3 Build (GTK 182 green, clippy GTK all-targets clean; debug APK 06:42)
- [x] 4 Review (round 1, /simplify; after the device test rounds 2 and 3, /simplify; see below)
- [x] 5 Specs, docs, list (check-specs green; no doc describes the ring's colours; OPEN-ISSUES has no item)
- [x] 6 Verify and hand over (GTK 182, core 1554, Android 296 green; clippy clean; debug APK 07:01:40 on the FP5, Librem 5 binary 06:53:15; Janek: "Sieht gut aus, passt")
- [x] 7 Ship (deltas merged, archived, check-specs green; commit after Janek's go)

## Review Focus
- Filled parts (fill, rung beads, prep ring) stay the full accent; only the faint colour changes.
- The page behind the ring is black in both apps; on another background the opaque faint colour would show as a dark ring (none exists).
- Light style on Linux: the running page stays black (`.running-view-dark`), so nothing changes there.

## 1. Linux
- [x] 1.1 `the_track_beside_a_hollow_bead_is_not_brighter`: draw a ring (no fill, one small hollow bead at 0.25) into a 200x200 cairo image surface painted black, with accent (0.2, 0.4, 0.8); the pixel on the track 6 px beside the bead's centre (and 9 px beside the start point) equals the pixel on the track at 0.75 (22, 100), within 1 per channel. `draw_bell_ring` takes the accent instead of the area. Expected: FAIL (brighter), then PASS. Ran: at 5 px the probe sat on the bead's black inside (1 off from rounding only), moved to 6 px (178, 94): failed 88 against 51, passes with the fix.
- [x] 1.2 Track and hollow outlines in the opaque faint colour.

## 2. Android
- [x] 2.1 `track: root.accent.mix(root.page, 0.25)` and hollow insides `root.page`, `page` bound to the running canvas's background; debug APK; MANUAL on the FP5.

## Review round 1
Fixed:
- /code-review: the test now checks its reference pixel is the track, not the page; compares within 1 per channel (cairo rounding); tasks text said 0.5, the probe is at 0.75.
- Spec reviewer: the start point is probed too (9 px beside it); with a see-through outline it fails 87 against 51.
- /code-review: Android names the page colour once (`page`), used by the track mix and both hollow insides.
Rejected:
- /code-review: opaque faint colour assumes the black page (GTK `.running-view-dark`, Android `#000000`): both running pages are black in every style, and the change is exactly that assumption; the GTK comment "The running page is black" already sat in `draw_bell_ring`.
- /code-review: `accent_rgb` keeps its unused widget parameter: outside this change, as decided before building.
- /code-review: Slint borders sit inside the bead, cairo strokes on its edge: older difference, invisible with one opaque shade.
Question for Janek:
- Spec reviewer: a hollow bead on the filled arc (a random bell's bead riding the fill; a slept-through one-off bell in overtime) used to show no rim on Linux (25 % over full accent is full accent) and now shows the faint rim. Android always showed it.
  Janek: "Blasser Rand überall" (the faint rim everywhere, as the spec says and Android already shows). No code change.
Round 1 fixes touch only the test and name Android's page colour once: no behaviour change, so no second review round.

## 3. Android, after the device test
Janek on the FP5: with spread on the bead moves jerkily while the arc is smooth. Cause: the arc glides between the 200 ms ticks (`animate shown-filled`), the bead's position took `bead.at` directly. Janek: fix it in this change ("In dieser Änderung").
- [x] 3.1 First built: a hollow bead whose `at` equals `filled` took `shown-filled`. Fix review (round 2, both reviewers): it stepped back when riding began, jumped ahead when it rang, could catch a still bead the fill landed on, and rebuilt core's riding rule in the shell. Replaced: every bead's position glides 200 ms linear like the arc (`animate at`); still beads don't move, a riding bead follows the arc's targets with the arc's timing, forward only. A new session with as many beads as the last reuses the rows (`put_rows`), only when no refresh ran between them (otherwise the ring view is gone between sessions); then its beads glide 200 ms from the old places, as the arc glides from the old fill. MANUAL on the FP5. APK 07:01:40 installed.

Round 2 rejected:
- /code-review: `page` has no default: one caller, which sets it.
- /code-review: `mix` with a brush argument may not compile: it compiled (APK 06:57:56).
- /code-review: GTK's black and `accent_rgb`'s widget parameter, test probe geometry: as in round 1.

## /simplify
Applied: the faint colour is one closure setting the source (no tuple taken apart twice); GTK comment names the CSS class that makes the page black; Android `page` is an `in property` bound to `running-canvas.background` instead of a second `#000000` (reuse and altitude agents).
Skipped: a shared `PAGE` const in GTK (two lines, one function); `[u8; 4]` instead of `Vec` in the test (cosmetic); efficiency agent found nothing; altitude agent: the depth is right (a group painted at 25 % gives the same pixels with more code).

## Test steps
- Librem 5 or laptop: Timer with a bell every 5 seconds, start. The faint ring is one even shade all round; no brighter bits beside the hollow beads or the start point. A bead riding the fill (random bell) shows the faint rim on the fill (Janek's choice).
- FP5: the same; the beads' inside is black like the page, their rim the track's shade.
- FP5, bell every 5 s with spread: a bead riding the fill moves as smoothly as the arc's front and stays on it.

Artifacts: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (07:01:40, installed on the FP5); Librem 5 binary (l5.sh cross-build 06:53:15, md5 7cf7db9c…, copied).

## Commit message (proposed)
gtk, android: the ring's faint track is one even shade

The faint track and the outlines of hollow beads were the accent at
25 % alpha, so where an outline crossed the track the two layers made a
brighter spot, and on the phone the outline on a bead's grey inside was
a shade lighter than the track. The faint colour is now opaque, the
accent at 25 % on the black running page, and the phone's hollow beads
are black inside like the page.

On the phone a random bell's bead riding the fill jumped on every tick
while the arc glided; each bead now glides like the arc.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs

## Review round 3 (fix review of 3.1)
Spec reviewer: PASS; two notes on tasks.md (test step for the riding bead, APK times) fixed.
/code-review, rejected:
- Bead count changes (a late random ring drops a planned ring past the end; Add in overtime) rebuild the model, so beads jump once: rare, one jump where before every tick jumped.
- A rung bead's place is its ring time, which after a late tick can sit a little behind the fill's step: core's placement, before this change it jumped back, now it glides the same distance.
- Same bead count in a new session: the ring view is gone between sessions (`ring-on` false after a dismiss), see 3.1.
- An animated binding per bead: it runs only when that bead's place changes; still beads cost nothing.
- `page` default, GTK's black, test probe geometry: as in rounds 1 and 2.

## /simplify after round 3
All four agents: nothing to change in the bead line (same idiom as `shown-filled`; animating x/y would cut a chord; the repeated `200ms linear` is too small for a shared constant). Suites unchanged since (no Rust touched): GTK 182, core 1554, Android 296 green; APK 07:01:40 built with the line.
