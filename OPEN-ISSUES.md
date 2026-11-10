# Open issues (2026-10-09)

One list for everything still open: the Android bugs (formerly ANDROID-BUGS.md) and the refactors left from the audit (formerly REFACTOR-AUDIT.md). Done items are removed; the git history has both old files (`git show 6e81927:ANDROID-BUGS.md`, `git show 6e81927:REFACTOR-AUDIT.md`). Bug numbers stay as they were, because commit messages refer to them.

**Ground rules:**
- Decisions go in core.
- No DB schema or sync wire-format change without a migration, and nothing touches the Android build environment (Cargo.toml, Cargo.lock, build.rs, main.rs, gradle, the manifest, rust-build.sh) without asking.
- msgids stay unless the item says otherwise; new ones get all nine translations.
- TDD, then a phone check before the commit.
- Before the next release: run `build-aux/repro-probe.sh`.

## Batches, in suggested order

From three fresh-eyes audits on 2026-10-09 (code reading plus small GTK, GStreamer and SQLite probes; every finding of the third audit was re-checked by a second agent; nothing tried on a device). Numbers continue after the old list (#1 to #38), so commit messages stay unambiguous. Mark a batch `[x]` when it is committed.

| Done | Batch | Items | Why this place |
|:---:|---|---|---|
| [x] | A. Sync data safety | #40, #66, #69, #43, #88, #68, #90, #94 | data loss across devices |
| [x] | B. GTK session safety | #135, #129, #128, #67, #131, #134 | GTK loses or ends sessions |
| [x] | C. Presets and bell settings | #111, #125, #112, #71, #102, #48, #82, #133 | settings that change silently on other modes or devices |
| [x] | D. Backups and import | #41, #105, #49, #109, #110, #62 | backups that double or will not restore |
| [x] | E. Audio files | #54, #106, #93, #108, #107, #76 | broken, oversized or orphaned audio files |
| [x] | F. Sync account and status | #42, #70, #89, #95, #96, #97, #44, #83 | sync that stops silently or leaks the password |
| [x] | G. Labels, names and Undo | #74, #75, #86, #101, #103, #104, #122, #158 | names that differ per device, saves that fail |
| [x] | U1. /spec groundwork (before H) | #161 | every later batch leaves specs and gets a review |
| [x] | H. Timer engine | #39, #45, #46, #47, #72, #113, #166, #185 | wrong minutes, bells at wrong times |
| [x] | I. GTK timer and bells | #73, #126, #127, #77, #130, #53, #132 | GTK controls and bells misbehave |
| [x] | J. GTK Log | #117, #118, #119, #120, #121, #123 | the Log shows wrong rows and totals |
| [x] | K. Failed writes and stale pages | #52, #80, #78, #84, #124, #136, #140, #141, #170 | failures that look like success, stale screens |
| [x] | K2. GTK settings that don't stick | #191 | a setting reverts on the next start, unnoticed |
| [ ] | K3. Presets all or nothing | #193 | a preset that fails halfway |
| [x] | K4. GTK freeze before bells | #199 | the window hangs before every bell |
| [x] | L. Android platform (manifest: ask first) | #50, #51, #57, #79, #63, #149, #176 | other apps, phone transfer, audio focus, battery saver |
| [x] | M. Android layout and editors | #139, #142, #143, #144, #145, #146, #189 | hidden controls, lost edits |
| [x] | N. Stats numbers | #55, #59, #60, #114, #115 | screens disagree |
| [x] | O. Dates in other languages | #151, #152, #153, #154, #116 | wrong date order and labels |
| [x] | P. GTK translations | #56, #81, #64, #155, #156 | mixed languages |
| [ ] | Q. Sync efficiency (after G and J) | #92, #98, #99, #100, #137 | wasted network, disk and rebuilds |
| [x] | R. Tests | #91, #138, #150 | tests that cannot catch the bug |
| [ ] | S. Cleanup | moved to REFACTORING.md | dead code, wrong comments |
| [ ] | T. Text that doesn't fit | #159 | information the user can't see |
| [x] | U2. Docs for contributors | #163 | stale docs an AI trusts |
| [ ] | V. Randomised sync test | #162 | sync bugs that only two interleaved devices show |
| [ ] | W. Sync that stops on one bad item | #164, #165, #172, #173, #184, #188, #192 | one bad file or event blocks sync |
| [ ] | X. Release pipeline (ask first) | #168, #169, #201, #209 | builds that aren't what F-Droid or users get |
| [x] | Y. Slint library fixes (build environment: ask first) | #148, #160 | vendored Slint code; needs the repro probe |
| [ ] | Y2. Slint text fields (upstream) | #196, #198, #210 | a long press that selects nothing, a key that does nothing |
| [ ] | AA. Library major versions (Android ones: ask first) | #204-#208 | crates stuck on an old major get no more fixes |
| [ ] | Z. Android touch targets | #147 | controls under 48 dp; needs some UI rework |
| [x] | BB. Found in the Starred Files test | #211, #212, #213 | small ones (#213 was already fixed on beta) |

---

## K3. Presets all or nothing

### #193 Core: applying a preset is not one transaction
- Where: `meditate-core/src/preset_config.rs` `apply` (comment above it: core::db has no nestable transactions; `set_interval_bell_enabled` opens its own); the length is written after it by each app (`set_timer_session_secs` / `set_breathing`).
- Problem: a write that fails partway (sync lock, full disk) leaves part of the preset. Since K2 both apps say "Save failed" and re-apply the setup from before, but that can fail under the same lock, and then the page shows the half-applied setup.
- Fix: nestable transactions (savepoints) in core::db, then `apply` with the length inside one transaction; the apps drop the re-apply. Check what the sync event log records for a rolled-back apply.

## K4. GTK freeze before bells

## L. Android platform (manifest: ask first)

## N. Stats numbers

## O. Dates in other languages

## P. GTK translations

## Q. Sync efficiency (after G and J)

### #92 Every pulled event is uploaded again
- Where: `meditate-core/src/db/events.rs:270-282` (the INSERT leaves `synced` at its default 0, `schema.rs:245`); push `orchestrator.rs:420`.
- Problem: a device pushes back everything it pulled. A new phone joining a remote with 4,000 events uploads them all again; after Wipe Local a device pushes the whole remote log back. The batch count doubles, so compaction runs twice as often. No case needs this forwarding: compaction serialises all events, and Push local and #89 flag the rows.
- Fix: set `synced = 1` on the replay path only (`append_event_returning_newness` is shared with `emit_event`). Reword the comment in `tests/sync_roundtrip.rs:857`.

### #98 GTK rebuilds every view after every sync
- Where: `meditate-gtk/src/application.rs:617-648`.
- Problem: every write starts a sync, and every sync re-runs all Stats queries and resets the Log to page 1, even when nothing came in. Deleting a session deep in the Log jumps the list back a second after the delete commits.
- Fix: in `drain`, `changed |= r.as_ref().map_or(true, |s| s.brought_changes())`, and refresh only when true (as Android does, `ui.rs:203`). Do this after #118, #121 and #122: today this rebuild hides those bugs for sync users.

### #99 Going back to an older build leaves a cache marker that blocks a later replay
- Where: `meditate-core/src/db/events.rs:198-205`.
- Problem: `maybe_walk_events_for_cache_upgrade` returns early when the stored version is higher, so an older build leaves the newer marker. Events of a new kind that the older build recorded are never applied when the newer build comes back. Linux only (Android refuses a versionCode downgrade).
- Fix: when `stored > CACHE_SCHEMA_VERSION`, write `CACHE_SCHEMA_VERSION` and return. One test next to `re_open_at_current_cache_version_does_not_re_walk`. Must ship before the next `CACHE_SCHEMA_VERSION` bump.

### #100 Every sync attempt runs the full database open with an integrity check
- Where: `meditate-core/src/db/mod.rs:253-317` (quick_check at 296-305); gtk `sync_runner.rs:109`; android `sync_runner.rs:102`.
- Problem: each attempt runs the schema setup, a `user_version` write (which takes the write lock before the sync busy timeout is set) and `PRAGMA quick_check`, which reads every page. A failing quick_check counts as "ok" and is never logged.
- Fix: run quick_check once from the startup opens (gtk `application.rs:136`, android `ui.rs:9673`) and log its error. Update the `open()` docs (mod.rs:160, 186).

### #137 Android: a sync that fails before reaching the server reloads every screen
- Where: `meditate-android/src/ui.rs:197-203, 2475-2493`; `sync_runner.rs:107-116`; test `app.rs:1562`.
- Problem: every error counts as "maybe changed". With the password missing (#70), each save triggers a sync that fails at once, and the next tick reloads the Log to its first 15 rows and re-runs Stats.
- Fix: `Err(e) => !matches!(e, Unconfigured | PasswordMissing | OpenDb(_))`.

## R. Tests

## T. Text that doesn't fit

### #159 Text cut off or shortened with "…", found one screenshot at a time
- Rule (Janek, 2026-10-10): every text shows in full at phone width (360 px) in all nine languages. Cut off is not acceptable, and neither is "…": both hide information.
- Seen so far: Android's guided-import dialog title ("Geführte Meditation importi", `main.slint` guided-import card, a 22 px Text with no wrap); GTK's import toast (fixed in batch E with a shorter text); Android's import-question buttons (stacked in batch D); GTK's Type rows squeezing their choices (pinned in batch C).
- Android: Slint clips a Text that neither wraps nor elides, silently. 142 Text elements, 31 wrap, 10 elide. Check 1, a test: every Text showing translated words wraps (`wrap: word-wrap`) and has room to grow; `overflow: elide` is not allowed for them either. An explicit short list is exempt (numbers, the big timer, single symbols).
- Both apps, check 2, a test: slots that can't wrap (buttons, toasts, toggle and tab labels, dialog buttons, header titles, GTK row titles that ellipsize) have every translation measured with the real font at its real size against the slot's width at 360 px, and the test lists each one that doesn't fit. The work is the list of fixed slots and their widths; measuring needs nothing new (Pango in the GTK tests, the same font files for Android).
- Fix what the first run lists: wrap, give the slot room, or shorten the translation; never "…". Changing how buttons are arranged (stacking them, moving them) is the last resort and needs Janek's yes each time (2026-10-18).

---

## V. Randomised sync test

### #162 Sync tests only cover the interleavings someone thought of
- Where: `meditate-core/src/sync/` tests (`FakeWebDav`, `Sync::new`), `db/events.rs` replay.
- Problem: most sync bugs so far (#40, #101 and others in batch A) needed two devices editing in an order no hand-written test tried. `label_name_collision_is_idempotent_on_replay` replayed in the same order on both sides and missed #101.
- Fix: one test in core: two or three in-memory devices share one `FakeWebDav`; a seeded xorshift64 (the one bell jitter already uses, no new crate, so Cargo.lock stays untouched) picks a few hundred random steps: create, rename, delete or merge a label, preset, guided file or vibration pattern, add, edit or delete a session, change a setting, sync one device, compact. At the end every device syncs twice and all must hold identical data (compare sorted rows of each synced table). Run a fixed list of seeds in `cargo test`, and print the seed and the steps on failure so it reproduces. Optional: an `#[ignore]`d variant that runs many seeds for a manual soak.
- Then: a short "sync rules that always hold" section in EVENTS.md (devices converge; a delete at the same or later time wins; same names settle on the smaller uuid), each pointing to its test.
- Every failure it finds is a real bug: fix it in the same batch with its own small regression test.

## W. Sync that stops on one bad item

Decided 2026-11-03: each skipped item stays visible in the sync status that the sync symbol opens (popover on Linux, dialog on Android, since the change sync-status-on-tap), never in the log only. #173 waits for the next metainfo change; #192 waits until it happens again on ureq 3.

### #164 A batch this build can't read or apply stops every later pull
- Where: `meditate-core/src/sync/orchestrator.rs:311-312` (an unparseable batch is fatal and never recorded as known); `db/sessions.rs:882, 889-900` writes an unknown `mode` straight into the CHECK column (`schema.rs:151`; the same for bell `category`, interval-bell `kind`, `signal_mode`, `chart_kind`); `db/events.rs:717-749` replays the whole pull in one transaction.
- Problem: since #43 the device still uploads, but it never pulls again. A damaged batch file on the server never heals, and a newer peer's new mode or category blocks every later event from every device until this device updates; the status stays red.
- Fix: decide first whether version skew should keep blocking on purpose. Then: an unparseable batch is logged and recorded as known; a value outside the CHECK set skips that event's recompute (the event stays recorded, as unknown kinds are) so a later build's cache walk applies it. One test per case.

### #165 One audio file that fails to download stops the upload
- Where: `meditate-core/src/sync/orchestrator.rs:757, 912` (`Err(e) => return Err(e.into())`), `500` (WebDAV errors skip push); `db/error.rs:103-115` (no length cap on ids).
- Problem: a flaky link or a server error on one bell or guided file fails the pull, so push never runs and new sessions stay on that device. A uuid longer than 255 bytes makes `<uuid>.<ext>` too long for the file system, so every sync fails at the same file.
- Fix: for a per-file GET or write error, log it, leave the uuid unknown and continue, as `NotFound` already does (keep `Unauthorized` fatal). Cap `target_id` length in `target_id_is_well_formed_for` (e.g. 128). One test each.

### #172 A long Line vibration pattern overflows u32
- Where: `meditate-core/src/vibration.rs:145`; replay `db/vibration_patterns.rs:358` (`as u32`, no clamp); editor cap `meditate-gtk/src/vibration_editor.rs:34` (10 s).
- Problem: above about 463 s, `(2*i+1)*duration_ms` overflows: a panic in debug, a wrong envelope in release. Only a corrupt or hostile synced pattern gets there, because the editors cap the length.
- Fix: compute in u64 and clamp `duration_ms` to the editor's range on replay. One test.

### #173 The release-notes parser is fragile
- Where: `meditate-core/src/release_notes.rs:24, 63-64, 74-77, 91-93`.
- Problem: the About release-notes parser escapes `<em>`/`<code>`, misses `<p class=...>` and treats `type='development'` as stable; latent while the metainfo stays plain.
- Fix: a test per markup case when the metainfo next changes.

### #184 Saving an unchanged vibration pattern overwrites another device's edit
- Where: `meditate-core/src/db/vibration_patterns.rs:266-312` (`update_vibration_pattern` always writes and emits).
- Problem: the same as #103 for the other kinds: the phone edits a pattern, the laptop opens the editor on the old version and saves without changing anything; after sync both show the laptop's old version.
- Fix: return early when name, duration, intensities and chart kind equal the stored row. One test.

### #188 A session event that isn't JSON can stop the database from opening
- Where: `meditate-core/src/db/schema.rs` (`ADDED_COLUMNS` fill for `sessions.guided_file_name`), `db/sessions.rs` (`recompute_session`, the named-event lookup), `db/labels.rs:366`; `append_event` stores payloads unchecked.
- Problem: `json_extract` fails on a payload that isn't JSON. In the one-time fill that fails `Database::init` on every open (the fill rolls back and retries). No known writer produces such a payload; found by the second review of f4c6b81.
- Fix: `CASE WHEN json_valid(payload) THEN json_extract(...) END` at all three places; one test with a broken payload in the log.

### #192 GTK: a sync can wait minutes after its push, past the 60 s read limit
- Seen: laptop, 2026-10-14 11:15:55, during batch K's device test (after deleting a guided file). The log ends at `[sync.push] ok events=1 sounds=0 guided=0` with no `sync.done`; the spinner kept turning. The worker thread sat in `recvfrom` on the Nextcloud TLS connection, which had neither sent nor received anything since the push (`ss`: lastsnd/lastrcv equal, over 3 min), although `webdav.rs` sets `timeout_read(60 s)`. The next step after the push is `maybe_compact_events`. At 11:19:50 it ended on its own without an error: `sync.done ... in 235.95s`, so a read waited about 4 min although each read is capped at 60 s.
- Problem: while one sync waits, the in-flight flag stays set and no later sync runs; the user sees a spinner for minutes. If the server never answers, it may never end.
- Ruled out (2026-11-03, with the ureq 3 port): a plain-HTTP read on a fresh or reused connection. `webdav.rs` tests `a_server_silent_after_the_headers_ends_the_request` and `a_server_silent_on_a_reused_connection_ends_the_request` end within the read limit on ureq 2 and on ureq 3; the TLS path is unchecked.
- Fix: find where it blocks, most likely a read inside TLS (test with a self-signed certificate that only the test client trusts); then bound it and test that.

## X. Release pipeline (build environment: ask first)

### #168 A release APK can build without its reproducibility pins
- Where: `meditate-android/android/rust-build.sh:41-50, 76`.
- Problem: a missing platform 34 prints nothing and a missing build-tools 31.0.0 only warns; both exit 0 with a signed APK that F-Droid's check rejects. RUSTFLAGS is a space-joined string, so a repo path with a space breaks the remap flags.
- Fix: for `PROFILE=release`, fail when either pin is missing (debug keeps the warning); use `CARGO_ENCODED_RUSTFLAGS`. Build environment: ask first, then run the repro probe.

### #169 Release CI: unpinned actions and images, and a light run can be picked
- Where: `.github/workflows/flatpak.yml:27-181` (`@v6`/`@v7`, image by tag, `tonistiigi/binfmt` untagged and privileged at 146); `build-aux/attach-flatpaks.sh:24-26`.
- Problem: whoever controls those tags can change what Linux users install. attach-flatpaks takes any successful run for the commit, including a `scope=light` run with no bundles (`flatpak.yml:107, 131`), and then fails at the download without saying why.
- Fix: pin each `uses:` to a commit SHA and each image to `@sha256:`; in attach-flatpaks, pick a run that has both artifacts.

### #201 Android: targets API 34, and the native library isn't built for 16 KB pages
- Where: `meditate-android/android/app/build.gradle` (`compileSdk 34`, `targetSdk 34`, `ndkVersion '27.2.12479018'`); the platform 34 pin in `rust-build.sh`.
- Problem: Google Play takes new apps only at the current target level (35, probably 36 by now) and with native code aligned for 16 KB memory pages, which NDK 27 doesn't do by default. F-Droid doesn't require either yet, but a stale target makes Android treat the app as legacy and makes a later Play listing a build-environment change first.
- Fix: check Google's current required level, raise compileSdk/targetSdk to it, and build for 16 KB pages (NDK 28, or `-z max-page-size=16384` for the Rust library). Re-test on the phone what the new target changes (notifications, the foreground service, exact timing), then run the repro probe and update the pins. Build environment: ask first. Raised while weighing a Play Store listing, 2026-11-02.
- Same change could end the JDK 17 pin: it exists because the d8 from build-tools 31.0.0 (F-Droid's buildserver) rejects classes from a newer javac, and AGP 7.3 stops at JDK 17. A newer AGP brings its own pinned d8, so both sides match again; needs a fdroiddata MR to the recipe (newer JDK) landing with that release.

### #209 The pinned toolchain is behind, and two crates wait for it
- Where: `rust-toolchain.toml`, `build-aux/setup-android.sh` (`PINNED_NDK`, `PINNED_GRADLE`, `PINNED_KOTLIN`), `meditate-android/android/build.gradle` (Kotlin plugin), the Gradle wrapper, the CI APK's own JDK 21 and NDK (`.github/workflows/flatpak.yml:73-81`, and its JDK rule in `.github/renovate-config.json5`); F-Droid's recipe pins Rust and the NDK as well.
- Problem: the update list (#21) shows newer Rust, NDK patches (r27d), Gradle and Kotlin; AGP and NDK 28 are #201. Two crates are held in `Cargo.lock` for them: kstring 2.0.2 (2.0.5 needs Rust 1.96) and cc 1.2.60 (it compiles the APK's C code; only F-Droid's own rebuild can show that a newer one keeps the bytes).
- Fix: one toolchain step at a time, each with the repro probe, and a fdroiddata MR that changes the recipe in the same release; then lift the two holds (`cargo update -p kstring -p cc`). Until then a bare `cargo update` moves them too: re-hold both with `--precise` afterwards. For kstring, `rust-version = "1.95"` on the workspace crates (meditate-gtk still says 1.75) with Cargo's `incompatible-rust-versions = "fallback"` would hold it and any later crate by itself. Build environment: ask first. Found with the first update list, 2026-11-03.

## Y2. Slint text fields (upstream)

### #196 Android: a long press on a field that has no focus yet selects nothing
- Where: Slint's Android backend (`long_press_timeout` only acts on the focused item) and core `TextInput` (on Android it takes focus on release, `items/text.rs` Released branch). Not our code.
- Problem: long-pressing a word in a field you haven't tapped yet selects nothing; you have to tap first. Android's own fields select the word. Found in the review of #195 (2026-10-18).
- Fix: upstream; the long press would need to focus the pressed field first. Report it there.

### #198 Android: Gboard's "line start" key doesn't move the cursor
- Where: Slint's Android backend, not our code. It maps `KEYCODE_MOVE_HOME` to Home (`androidwindowadapter.rs` `map_key_code`), so the cause is unknown.
- Problem: in Gboard's text editing panel, the |< key left the cursor where it was in a Slint text field; the arrow keys and Select worked. Seen once in the material gallery on the API 35 emulator (2026-10-19), while testing Slint PR #14205; not reproduced, not tried in Meditate or on the FP5.
- Fix: first reproduce it (with and without Select), log which key event or `setSelection` arrives, then fix or report upstream.

### #210 Android: the word under the cursor flickers in the accent colour and changes by itself (GitHub #23)
- Where: Slint's Android backend, not our code. `callback_update_text` (`third_party/i-slint-backend-android-activity/javahelper.rs`) turns the keyboard's composing region into a TextInput preedit: it shows as a selection and moves the cursor to the word's end, the keyboard re-composes, and the loop repeats; Gboard may then insert a suggestion.
- Problem: tapping into a field (sync server URL, session note) highlights a word, flickers and can change it without any typing. Reported in GitHub #23 with a video.
- Fix: upstream in Slint PR #14216 (merged 2026-11-07, after 1.18.1). Decided 2026-11-07 to wait for the release that carries it, then update Slint (see `third_party/README.md`), test on the FP5 and close GitHub #23. A local minimal port (always commit the whole text, drop the preedit branch) was weighed and not taken.

## AA. Library major versions (Android ones: ask first)

Found with the first update list (#21), 2026-11-03. Each is its own change: read the crate's changelog, adapt the code, run the suites, and test on a device. The ones in the Android build change the APK, so they need a go and the repro probe.

### #204 jni 0.21 -> 0.22 (Android)
- Where: every JNI call in `meditate-android/src/`.
- Problem: one major behind on the bridge to the Kotlin services.
- Fix: test every Android feature that crosses JNI on the phone (sessions in the background, bells, vibration, the keychain, the widget, file picking).

### #205 oo7 0.4 -> 0.6 (Linux)
- Where: `meditate-gtk/src/keychain.rs`, `meditate-gtk/Cargo.toml`.
- Problem: two minors behind (0.x, so breaking) on the keyring client that stores the Nextcloud app password.
- Fix: test that an existing stored password is still found after the update, in the Flatpak and the native build.

### #206 gettext-rs 0.7 -> 0.8 (Linux)
- Where: `meditate-gtk/src/main.rs`, `meditate-gtk/src/i18n.rs`, `meditate-gtk/Cargo.toml`.
- Problem: one breaking release behind on the Linux translations.
- Fix: run `cargo test -p meditate i18n::` and start the app in two languages.

### #207 roxmltree 0.20 -> 0.21 (Android too)
- Where: `meditate-core/src/sync/webdav.rs` (WebDAV PROPFIND replies), `meditate-core/Cargo.toml`.
- Problem: one breaking release behind on the XML parser for server replies.
- Fix: the sync tests cover the parsing.

### #208 material-colors 0.4 -> 0.5 (Android)
- Where: `meditate-android/src/theme.rs`, `meditate-android/Cargo.toml`.
- Problem: one breaking release behind on the library that turns the system colour into the Material palette.
- Fix: compare screenshots of light and dark mode before and after.

## Z. Android touch targets

### #147 Android: small touch targets
- Where: `meditate-android/ui/main.slint:641, 701, 1277, 6353, 1782, 1799`.
- Problem: the mode switch, the Stats period toggle and the Sound/Vibration/Both segments are 32 dp tall and the Box Breath steppers 36 dp; Material asks for 48.
- Fix: taller controls or taller TouchAreas (the PhaseTile row is already full width at 360 dp, so it can only grow taller).

## Later

Ideas and larger improvements, not bugs; the maintainer decides if and when.

### #177 Applying a preset is not atomic
- Where: `meditate-core/src/preset_config.rs:485-` (each write in its own transaction, `db/settings.rs:25`); callers gtk `timer/imp.rs:3498`, android `ui.rs:1288`.
- Problem: a write failing partway (busy database, full disk) leaves settings, interval bells and box-breath phases out of step with the preset.
- Fix: a savepoint helper (`SAVEPOINT` / `RELEASE` / `ROLLBACK TO`, keeps `&self`) and `db.run_in_transaction(|db| apply(...))`.

### #178 GTK: the Done note field should grow instead of scrolling in 180 px
- Where: `meditate-gtk/data/ui/timer_view.blp:836-841`.
- Fix: drop the inner ScrolledWindow and `height-request`, keep `note_view` (the page already scrolls), and check that `log-note-editor` still draws its background. Android already grows.

### #179 Bundled Box Breath voice cues
- Where: `meditate-gtk/data/sounds/` + gresource, `meditate-android/assets/sounds/` + `src/sounds.rs`, `meditate-core/src/db/seeds.rs`.
- Idea: four short CC0/CC-BY cues ("Inhale", "Hold", "Exhale", "Hold") as box-breath seeds, so the phase chooser isn't empty. The seed is one-shot (`seeds.rs:27-29`), so existing installs need a seed-version step.

### #180 Warn about a missing bell sound before Start
- Where: rows already say "Missing" (`meditate-core/src/bells.rs:304`, gtk `bells.rs:384`, android `ui.rs:3224`).
- Idea: a core check over the active bells; a warning style on the row, a Setup banner, and block or confirm Start, so a bell isn't silent mid-session. Both apps; vibration patterns the same.

### #181 Daily goal history
- Idea: a `goal_history(effective_from, minutes)` table instead of the `daily_goal_mins` setting; on change, ask "from today" or "retroactively"; the heatmap and goal ring look up the value per day. Synced through events; needs a migration seeded from the current setting. Both apps.

### #182 Mindfulness bell during the day
- Idea (tentative): a tab that rings at intervals during set hours. Prototype background scheduling first (Android exact alarms; Linux notifications or feedbackd).

### #187 Android: a Local Folder the system hides shows as empty, without a word
- Where: `meditate-android/kotlin/MeditateLocalFolder.kt` (`walk`, an empty children cursor counts as "ok"); `meditate-core/src/db/local_guided_files.rs` (`apply_local_scan` marks every file missing).
- Problem: seen on the FP5 (Android 15) 2026-10-12: the system's ExternalStorageProvider failed its restricted-path check ("NoSuchFileException: /storage/emulated", "Queried directory ... is hidden") and returned no children. Meditate showed "0 files" and marked all files not found; nothing said that Android, not the folder, was the problem. A phone restart fixed it. Stars and plays were kept.
- Idea: when a scan of a folder that had files comes back with none, keep the files as they were and show the "Folder not reachable" banner with Choose Again, instead of marking all missing. A really emptied folder then needs one confirmation.

### #183 Nextcloud with a self-signed or private-CA certificate can't connect
- Where: `meditate-core/src/sync/webdav.rs` (`HttpWebDav::with_read_timeout`, the Agent config; ureq 3 with its built-in webpki roots only).
- Idea: ureq 3's `TlsConfig` takes other roots: the system store (`RootCerts::PlatformVerifier`, its `platform-verifier` feature) or one pinned certificate. A new feature or crate changes Cargo.lock: ask first.

### #190 The day ends at midnight, also for night owls
- Where: every "today" in core (`time::today_local`, goal, streak, heatmap, Log day groups) counts by calendar day.
- Idea: a "day starts at" time (for example 3:00), so a 1 a.m. session counts for the day the user lived. It changes counting in both apps (goal, streak, heatmap, Log groups, synced sessions on another device), probably as a setting. Raised during batch J, 2026-10-13.

### #200 Android: one test that every UI text is translated
- Where: `meditate-android/src/app.rs` checks single texts per issue (`nextcloud_help_is_translated`, `ui_text_is_translated_whole`); GTK already checks every msgid (`meditate-gtk/src/i18n.rs::every_language_translates_every_text`).
- Idea: one test that reads every `@tr("...")` in `ui/*.slint` and fails when a catalog lacks it or leaves it empty, so a new text can't ship untranslated without its own test. Existing gaps may show up first. Raised during GitHub #18, 2026-11-02.

### #194 Back up everything, not only the Log
- Where: Preferences backup in both apps; the CSV export (`meditate-core/src/data_io.rs`) carries sessions and labels only.
- Problem: without sync, presets, interval bells, Box Breath patterns, vibration patterns, imported sounds and guided files, and every setting live only on the device. A lost or reset phone or laptop loses them; the CSV restores the Log alone.
- Idea: a full backup file (the database, e.g. `VACUUM INTO`, which needs no new dependency, plus the sounds and guided audio) and a restore for it, in both apps. Open questions: what a restore does to the device id, the event log and sync state on a device that syncs, and whether it replaces or merges. Raised by the maintainer, 2026-10-18.
