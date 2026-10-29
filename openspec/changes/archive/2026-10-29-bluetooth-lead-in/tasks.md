# Tasks: bluetooth-lead-in

Base: 0c99e8d; suites on it: core 1392, android 267 green (run on this
tree before the 0c99e8d commit); GTK (`cargo test -p meditate`) 162 green.

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand (said back, Janek "passt"; decisions in proposal.md)
- [x] 2 Propose (fresh-agent check done, answered in Notes; approved: Janek 2026-10-29 "go")
- [x] 3 Build (core 1395, android 271, GTK 164 green; every new test failed first; Android-target, CI and GTK clippy clean)
- [x] 4 Review, then `/simplify` after the last review (review, fix review, /simplify; see Notes)
- [x] 5 Specs, docs, list (check-specs green; README Bells line; no OPEN-ISSUES entry)
- [x] 6 Verify and hand over (core 1395, android 271, GTK 164 green; CI, GTK and Android-target clippy clean; check-specs green; APK 21:15:01 installed, Linux build 21:15:30; FP5 and Linux: Janek 2026-10-29 "funktioniert gut auf linux und auf dem handy"; on the alarm channel the lead-in is quieter and works less well than on media (Android damps sonification on headsets and keeps the box's own alarm step); Janek: leave it, a longer length helps, wait for reports)
- [x] 7 Ship (deltas appended to openspec/specs/bells and preferences, archived 2026-10-29, check-specs green)

## Review Focus
- An Android bell aimed at a Bluetooth output that Android refuses to aim (`setPreferredDevice` false) on the track: the noise plays on the default route; the bell still starts at the marker.
- The track never reaching its marker (track failed to start, device gone): the bell starts anyway (or stops with the output change), never waits forever.
- A preview replaced during its lead-in; a session stopped during a lead-in; Back during a preview's lead-in.
- Two bells on the same output within one lead-in (interval and end): one noise, both after it. Bells on different outputs: each its own.
- Lead-in on, headphones are wired: no noise. On Linux a virtual sink in between (combine, filter): not seen as Bluetooth, no lead-in.
- The preview pill: shows Stop from the tap (the lead-in counts as playing), reverts after the bell.
- An old database: neither key, off and 100 ms. A synced value from the other device while a lead-in plays: the next bell uses it.
- Translations: the stash holds all strings in all nine languages for both apps except "Bluetooth".

## Notes
Builder check (fresh agent), nine questions; my answers (proposal decision 7):
1. Vibration: both apps delay it by the wait the player returns (planned, or what is left of a running lead-in); a Stop meanwhile drops it (a counter bumped by StopActiveSignals); a bell that never rings still vibrates. Changed while building from "Linux at the end of stream": one rule in both apps, no callback from the noise into the timer page.
2. A marker that never comes: the bell starts at the planned time plus 5 s (was 0,5 s; changed after the review, finding 2).
3. The noise lives while any bell waits on it; stopping a preview removes only that waiter; a new preview joins a running noise like any bell.
4. Linux watches no output change during a lead-in (only Stop); proposal says so.
5. Bluetooth sinks: "bluez" prefixes for PipeWire and PulseAudio; `is-default` verified on the laptop (PipeWire): the built-in sink reported `is-default: True`; the Flatpak unverified.
6. The noise at its fixed level, on the bell's step, not trimmed by the bell's gain; aimed at the device on both channels.
7. Test: a volume-preview style Play/Stop over noise and bell, in the preview slot, stoppable during the hiss; GTK reverts on the bell's end, not `notify::playing`.
8. A failed lead-in: only logged, the bell plays without it (Janek, proposal decision 8); no new string.
9. New keys `bluetooth_wake_lead_in` / `_ms`, because the first attempt's `bluetooth_lead_in` may be in synced data; rounding half up.
Traps: the stash's `.po` diffs delete the "Play Bells as Alarm" strings, so only the lead-in entries are copied; GTK has no "Play" string yet.

Built notes: `lead_in_ms(NaN)` → 100 was added with its fix (clippy's clamp warning), not watched failing first. GTK translations come from the Android catalogues (same msgids); `update-translations.sh` run, no fuzzy left. The Android Test reuses the volume-row Play/Stop pill (no new string); GTK got "Play" (from Android) and "Stop".

## Review (fresh agent)
FAIL on one missing point, NEEDS FIXES; my handling:
- Missing: Android has no preview-only stop (every preview stop calls `audio::stop`). Not built: previews never play during a session on Android (the running page hides navigation), so the case can't occur; the spec and proposal now say so. Linux keeps it.
1. IMPORTANT, fixed: `SIGNAL_GEN`'s `thread_local!` had split `dispatch_effects` from its doc (`unused_doc_comments` on the Android target); moved above the doc.
2. IMPORTANT, fixed: the fallback (planned + 0,5 s) could fire while a cold A2DP link still held the track; now planned + 5 s in both apps; the preview pill keeps asking while the player says it plays (finding 8 with it): `preview_playing` returns `None` when it can't be asked, and only then the length bounds the poll.
3. IMPORTANT, fixed: Linux logs a failed GStreamer init, a failed monitor and "no default sink found".
4. IMPORTANT, not changed: a failed lead-in is only logged, against the AGENTS.md rule; Janek's decision 8, to be confirmed with him.
5. MINOR, fixed: GTK `start` pauses a still-ringing preview before it waits.
6. MINOR, fixed: `parse_hundredths` sat under `end_button_code`'s doc.
7. MINOR, fixed: a refused aim on the noise track is logged; a short write fails the lead-in (bell at once).
8. MINOR, fixed with 2.
9. MINOR, skipped: typing then tapping +/- saves twice (harmless, one extra synced write).
10. MINOR, skipped: one cached WAV per length tried, never cleaned (about 1 MB at 5 s; tuning is rare).

Fix review (fresh agent, the fixes above): PASS, two MINOR, fixed: a preview replaced while it waited kept its row's Stop on Linux (`play_preview` now notifies like `stop_preview`); past its length the Android pill polled every 50 ms, now 250 ms. Left as is: a `start()` that throws in `wakeLocked` would keep the pill on Stop (a prepared player doesn't throw); a late joined lead-in's vibration fires early by the stall (rare).

/simplify (four agents), applied: one core reader `bluetooth_wake_lead_in(db) -> Option<u32>` for what a bell plays (both apps; Android frees the database before making the samples, GTK takes one lock); `lead_in::LATE_MS`, `MIN_MS`, `MAX_MS` in core (GTK reads them; Kotlin's `LATE_MS` and `LEAD_RATE` are held equal by tests); Kotlin `leadLocked` returns `Long?` instead of a 1 ms sentinel; GTK: the dead `play_starting_sound` deleted, `stop_preview` flatter, cached noise files of other lengths deleted, `app()` reused by the error toast, the woodblock id through `crate::db`, the Test button keeps its own state instead of reading its translated label. Skipped: a per-length cache of the Android samples (14 KB at the default); asking Kotlin for the output before sending them; keeping one GStreamer monitor open (10 to 100 ms before the noise, only while on); an "on start" callback for the vibration (Android has no Kotlin-to-Rust callback; it would need native registration, the build environment); merging the three JNI play invokes; one `lead` instead of a list in Kotlin (an output change mid-lead would join the old device's noise); GTK's immediate vibration arm (keeps the old exact path without a lead-in); typing then +/- saving twice.

## Tasks
- [x] 1.1 Core `settings_keys`: `BLUETOOTH_WAKE_LEAD_IN_KEY` ("bluetooth_wake_lead_in"), `BLUETOOTH_WAKE_LEAD_IN_MS_KEY`, readers with clamp and 10 ms rounding half up. Proof: `the_lead_in_is_off_and_a_tenth_of_a_second_until_set`.
- [x] 1.2 Core `lead_in`: `samples(ms, rate)`, `wav(ms)`, `is_bluetooth_sink(props)`. Proof: `the_lead_in_is_noise_then_a_short_silence`, `a_bluez_sink_counts_as_bluetooth`.
- [x] 2.1 Android player: `play`/`playBell` take the samples (empty for none) and return the wait; noise `AudioTrack` (static, the hold's attributes, preferred device, the hold raised first), `setNotificationMarkerPosition` at its last frame starts the bell, join per output, release on stop/finish/output change. Proof: `a_bluetooth_bell_waits_for_its_lead_in`.
- [x] 2.2 Android `ui.rs`: the samples when on (all play sites), vibration delayed by the returned wait; JNI table. Proof: `a_bell_vibrates_when_its_sound_starts`, `every_jni_call_matches_its_kotlin_method`.
- [x] 2.3 Android Preferences group, Test, nine `.po`. Proof: `the_bluetooth_group_holds_the_lead_in`.
- [x] 3.1 GTK `sound.rs`: default sink check via `gst::DeviceMonitor`, core's WAV as a `MediaFile`, bell on its `ended`, join, Stop; `timer/imp.rs` vibration waits. Proof: `the_lead_in_plays_before_a_bluetooth_bell`.
- [x] 3.2 GTK Preferences group, Test, nine `po/*.po`. Proof: `meditate-gtk/src/preferences.rs::the_bluetooth_group_holds_the_lead_in`.
- [x] 4.1 README Bells line.
- [x] 6.1 Devices (spec scenario "On the devices").

## Hand-over (step 6)
APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (21:15:01), debug, `adb install -r`.
Linux: `meditate-gtk/builddir/src/meditate` (21:15:30, `./build.sh gtk --debug`); before starting it, check no Flatpak Meditate runs (same app ID).

Steps (Android, the Bluetooth box connected):
1. Preferences: "Bluetooth" group after "Glocken": explanation in full, switch off, no length row; switch on: "Dauer (Sekunden)" slides in at "0,10"; "5,00" fits; typing "1,5" gives "1,50".
2. Switch off, box asleep (60 s silence), Test: the box swallows the woodblock (shows the problem).
3. Switch on, 60 s silence, Test: hiss, then the woodblock; raise the length until only the woodblock is heard after a wait; Test's button shows Stop during hiss and bell, and stops both when tapped during the hiss.
4. The same with "Glocken als Alarm abspielen" on: the phone speaker stays silent for the hiss and the bell.
5. A Timer session with a starting bell, interval bells and vibration: each bell after its hiss, the vibration with the bell; Stop during a hiss: no bell, no buzz.
6. Box Breath with sound cues: each cue after its hiss.
7. Without the box (speaker or wired): no hiss, bells at once.
Steps (Linux, laptop with the box as default output): 8. the same group after "Statistik"; Test and a session as in 3 and 5; another output: no hiss.

Commit message:
```
bells: Wake Bluetooth devices with a lead-in before each bell

Bluetooth headphones and speakers fall asleep after 30 to 60 s of
silence and swallow the start of the next bell, short bells entirely.
With the new wake-up lead-in on (Preferences, Bluetooth), a soft noise
plays for the set length before each bell on a Bluetooth output, then
0.05 s of silence counted from where the noise really ended, then the
bell; on both Android channels and on Linux. A Test row helps find the
length a device needs.

Refs #10
```
