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
| [x] | L. Android platform (manifest: ask first) | #50, #51, #57, #79, #63, #149, #176 | other apps, phone transfer, audio focus, battery saver |
| [x] | M. Android layout and editors | #139, #142, #143, #144, #145, #146, #189 | hidden controls, lost edits |
| [x] | N. Stats numbers | #55, #59, #60, #114, #115 | screens disagree |
| [x] | O. Dates in other languages | #151, #152, #153, #154, #116 | wrong date order and labels |
| [x] | P. GTK translations | #56, #81, #64, #155, #156 | mixed languages |
| [ ] | Q. Sync efficiency (after G and J) | #92, #98, #99, #100, #137 | wasted network, disk and rebuilds |
| [x] | R. Tests | #91, #138, #150 | tests that cannot catch the bug |
| [ ] | S. Cleanup | #58, #61, #85, #87, #157, #167, #171, #175, #197 | dead code, wrong comments |
| [ ] | T. Text that doesn't fit | #159 | information the user can't see |
| [x] | U2. Docs for contributors | #163 | stale docs an AI trusts |
| [ ] | V. Randomised sync test | #162 | sync bugs that only two interleaved devices show |
| [ ] | W. Sync that stops on one bad item | #164, #165, #172, #173, #184, #188, #192 | one bad file or event blocks sync |
| [ ] | X. Release pipeline (ask first) | #168, #169, #174 | builds that aren't what F-Droid or users get |
| [x] | Y. Slint library fixes (build environment: ask first) | #148, #160 | vendored Slint code; needs the repro probe |
| [ ] | Y2. Slint text fields (upstream) | #196, #198 | a long press that selects nothing, a key that does nothing |
| [ ] | Z. Android touch targets | #147 | controls under 48 dp; needs some UI rework |

---

## K3. Presets all or nothing

### #193 Core: applying a preset is not one transaction
- Where: `meditate-core/src/preset_config.rs` `apply` (comment above it: core::db has no nestable transactions; `set_interval_bell_enabled` opens its own); the length is written after it by each app (`set_timer_session_secs` / `set_breathing`).
- Problem: a write that fails partway (sync lock, full disk) leaves part of the preset. Since K2 both apps say "Save failed" and re-apply the setup from before, but that can fail under the same lock, and then the page shows the half-applied setup.
- Fix: nestable transactions (savepoints) in core::db, then `apply` with the length inside one transaction; the apps drop the re-apply. Check what the sync event log records for a rolled-back apply.

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

## S. Cleanup

### #58 Dead and duplicated code (core and Android)
- Core: `SessionPhase::Paused` (`session/mod.rs:121`) is never created; delete it and its 3 match arms. `daily_totals()` / `get_daily_totals_from_db` (`db/sessions.rs:172, 671-697`) reads the whole table for the 91-day heatmaps; call the `_since` version and delete it with its tests.
- Android: about 60 `let _ = weak.clone();` and 5 `let _ = current_mode.get();` (e.g. `ui.rs:4751`); `AppState::primary_label`, `remaining`, `is_idle` (`app.rs:392, 594, 616`) used only by their own tests; Kotlin `MeditateKeychain.clearPassword` has no caller; `service.rs::call` (87-104) repeats `jni_call::load_class`.
- More (third audit): gtk `keychain::delete_password` (141-156) with `Backend::delete` (306-319) and core `settings::clear_nextcloud_account` (`sync/settings.rs:77-85`) have no callers (also fix the comment at android `keychain.rs:63-67`). Android no-op lines `ui.rs:5994, 6067, 6077, 6089, 7201, 7245, 8744` and the stale `chooser_bell_volume` doc (9621-9623). `WidgetPreset::mode` (`widget.rs:45-50, 63-67, 189-199`) is written but never read: delete it and `mode_tag_is_carried`. `invoke_no_arg`, `invoke_player_noarg`, `invoke_refresh` and `service.rs::call` are one static `(Landroid/content/Context;)V` call four times: one `jni_call::call_context_void`.

### #61 GTK dead code and the duplicated transcode pipeline
- Dead: `populating` Cell never set (`bells.rs:604`, guards at 630, 647, 661, 740); unused `toast_slot` (`presets.rs:336/360`); unused `chart_h` (`stats/imp.rs:465/496`); unused `area` (`stats/imp.rs:697/707`); `let _ = BellSoundCategory::General` (`db/mod.rs:372`); half-deleted "Throwaway" comment (`timer/imp.rs:1170-1175`).
- Duplicate: `sounds.rs:735-849` and `guided.rs:433-534` are the same gst pipeline except `audioloudnorm` and the cancel-aware bus loop; one function with `loudnorm: bool` and `cancel: Option<&AtomicBool>`, about -80 lines.
- Update (third audit): `audioloudnorm` never runs. It is not in the GNOME 50 or 51 runtime and the manifest adds no GStreamer module (it comes from gst-plugins-rs, not plugins-bad). Delete that branch (`sounds.rs:756-776, 788-794`) and fix the comment at `guided.rs:426`; the merged pipeline then needs no `loudnorm` flag.
- More dead code in `timer/imp.rs`: the `starting_bell_sound` read with its `"bowl"` fallback, bound to `_starting_bell_sound_legacy` (3081-3083, 3101, 3110, 3137); `apply_preferred_label_for_mode(_mode)` (4267) ignores its argument; `build_breathing_setup` (4076) and `breath_elapsed` (3011) only pass through; 2397-2401 and 2605-2609 hand-copy `From<TimerMode>` (use `mode.into()`); `PKGDATADIR` (`config.rs:6-7`) has no reader and hides behind `#[allow(dead_code)]`. Remove `tick_mode` (194) only after #129.

### #85 Dead code, duplicates and a test that cannot fail
- Core: `Effect::EndPrep` is never consumed (effect.rs:21-25, mod.rs:460, asserts 1065/1077/1636, ARCHITECTURE.md); `tick_box_breath` calls `fire_due_bells` though Box Breath never has interval bells (mod.rs:597-604, test 1475); `db_open_failure_key` (format.rs:295-315) is unused and repeated in gtk `application.rs:168-177`; `Stopwatch` serde derives and tests (timer.rs:1-12, 86-113) are unused and assert a false restart claim; `box_breath_phases.rs:17-47` repeats `get_box_breath_phase` and hard-codes the seed uuids at 141-144; `list_bell_sounds_for_category` (bell_sounds.rs:236-264) copies `list_bell_sounds`; `count_labels_from_db` (labels.rs:68) is test-only.
- GTK: `preload_end_bell` builds a media file that is never played, `play_starting_sound` has no caller (sound.rs:250-301, calls at timer/imp.rs:688, 735, 1209-1212, window/imp.rs:111-118).
- Android: label rename/delete handlers repeat `refresh_after_label_change` (ui.rs:7700-7706, 7752-7764); `ui.rs:537-545` repeats core `bells::signal_mode_override_from_db`.
- Test: `bell_rng_state_advances_deterministically_from_seed` (mod.rs:1488-1520) uses `jitter_pct: 0`, so the RNG is never used; set 20 and assert a different seed differs, or delete it.
- Android (third audit): `validate_label_name` (2186) is `validate_rename_label_name(n, 0)` (2817); `bell_name_taken` (1175) is `bell_sound_name_taken(n, "")` (3715); the body of `read_global_setting` (3189-3193) is core `read_str` and its four `== "true"` callers are `read_bool`; the label-name map is built three times (609, 716, 763); 4007-4011 repeats `push_session_length_to_ui`; 2453-2458 repeats `refresh_setup_label_name`; `snapshot_setup_json` (1347-1371) rebuilds the timing that core `active_presets` builds (needs a small core function). About -30 lines.

### #87 Trimming the diagnostics log drops its 0600 permission
- Where: `meditate-core/src/diag.rs:165-185` vs `:77-87`.
- Problem: `trim_to_tail` writes the copy with `File::create` (0644) and renames it over the 0600 log. No secrets are logged today, and Flatpak/Android keep it private anyway.
- Fix: create the temp copy with `OpenOptions` and `.mode(0o600)`, as `init` does.

### #157 Wrong or misplaced docs and comments
- EVENTS.md:78, 89, 235: the batch file name is `<min_lamport:014>__<batch_uuid>.json`, the bell categories are general and box_breath, and there are 7 entity families plus 2 singletons. The same stale file name is at `orchestrator.rs:374`.
- gtk `db/mod.rs:113-117` says new bundled sounds reach existing installs, but the seed is one-shot (`seeds.rs:27-29`); the block also sits on the wrong item.
- android `ui.rs`: doc comments on the wrong item at 488-492, 803-822, 1128-1132, 2348-2350 (shows a start time with a UTC offset, which never exists), 2904-2909 and 9477-9481; false claims at 1406, 1444, 3874, 4028, 5878 and 5909.
- `sync/fake.rs:1-4` claims the fake behaves like Nextcloud (see #91).

### #167 Linux packaging: a Rust floor that's too low
- Where: `build.sh:72`; `meditate-gtk/Cargo.toml:6`; `clippy.toml` (`msrv`).
- Problem: build.sh accepts rustc 1.85, but the locked gtk4 0.11.2 and glib 0.22.5 need 1.92, so the build fails halfway; `rust-version` and `msrv` still say 1.75.
- Fix: raise the check, `rust-version` and `msrv` to 1.92 (Cargo.toml: ask first).

### #197 GTK: bell rows in a revealer warn on GNOME 51
- Where: `meditate-gtk/data/ui/timer_view.blp:431-470, 533-570` (sound, volume and pattern rows of the start and end bell, each in a `Gtk.Revealer` inside the list).
- Problem: libadwaita in GNOME 51 checks that rows sit directly in a `GtkListBox` and logs 18 warnings at every start ("AdwActionRow placed into GtkRevealer, can only be placed into GtkListBox"). GNOME 50 has no such check. Janek's test of the GNOME 51 build passed (2026-10-18); seen so far only in the log.
- Fix: show and hide the rows without a revealer (row `visible`), or ask Janek whether the slide is worth keeping another way.

### #171 The `synchronous=NORMAL` comment promises safety it doesn't have
- Where: `meditate-core/src/db/mod.rs:229-241`.
- Problem: "events are append-only and idempotent on re-sync" is only true for events already uploaded; a power cut can roll back a just-saved session that exists nowhere else.
- Fix: correct the comment; or, if wanted, `PRAGMA synchronous=FULL` around Save and Edit only.

### #175 Leftover config
- Where: `meditate-android/manifest.yaml` (xbuild leftover) and the comment at `meditate-android/Cargo.toml:76-80`; `meditate-gtk/src/meson.build:3-9` (`cargo_profile_args` set, never used); `build-aux/io.github.janekbt.Meditate.json:25-26` (two Notifications talk-names, likely surplus under the portal).
- Fix: delete them and check that notifications still show in the Flatpak. Cargo.toml and manifest.yaml are protected build environment: ask first.

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

### #173 Server text ends up in the sync status and log; the release-notes parser is fragile
- Where: `meditate-core/src/sync/webdav.rs:97` (whole error body), `:107` (full redirect Location), `:282` etc. (`into_string()`); stored by `sync/settings.rs:134-138`. `release_notes.rs:24, 63-64, 74-77, 91-93`.
- Problem: an error page (up to 10 MB) or a Location with user name and host goes into `last_sync_error`, the tooltip and the shared diagnostics log. The About release-notes parser escapes `<em>`/`<code>`, misses `<p class=...>` and treats `type='development'` as stable; latent while the metainfo stays plain.
- Fix: show the status code only and log a truncated, control-character-stripped excerpt (as `MalformedResponse` does with 512 chars). Parser: a test per markup case when the metainfo next changes.

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
- Fix: first find where it blocks (a body read on a pooled connection, ureq 2's timeouts on reuse); then make every request bounded (`AgentBuilder::timeout` as an overall cap), and test with a fake server that answers the headers and then goes silent.
- Also seen: every pull skips the server file `00000000021265__7b91feb1-….json.tmp` as unparseable (a leftover from an interrupted upload by some device); nothing removes it.

## X. Release pipeline (build environment: ask first)

### #168 A release APK can build without its reproducibility pins
- Where: `meditate-android/android/rust-build.sh:41-50, 76`.
- Problem: a missing platform 34 prints nothing and a missing build-tools 31.0.0 only warns; both exit 0 with a signed APK that F-Droid's check rejects. RUSTFLAGS is a space-joined string, so a repo path with a space breaks the remap flags.
- Fix: for `PROFILE=release`, fail when either pin is missing (debug keeps the warning); use `CARGO_ENCODED_RUSTFLAGS`. Build environment: ask first, then run the repro probe.

### #169 Release CI: unpinned actions and images, and a light run can be picked
- Where: `.github/workflows/flatpak.yml:27-165` (`@v5`/`@v6`, image by tag, `tonistiigi/binfmt` untagged and privileged at 137); `build-aux/attach-flatpaks.sh:24-26`.
- Problem: whoever controls those tags can change what Linux users install. attach-flatpaks takes any successful run for the commit, including a `scope=light` run with no bundles (`flatpak.yml:107, 131`), and then fails at the download without saying why.
- Fix: pin each `uses:` to a commit SHA and each image to `@sha256:`; in attach-flatpaks, pick a run that has both artifacts.

### #174 build.sh fails for Android on a machine without rustup
- Where: `build.sh:167` runs `build-aux/setup-android.sh` before `ensure_rust` (`:171`); `setup-android.sh:300-303` exits when rustup is missing.
- Problem: README promises that build.sh installs missing dependencies, but a fresh machine stops with a rustup error on `./build.sh android`.
- Fix: call `ensure_rust` before the Android bootstrap. README/BUILDING say "install rustup first" until then.

## Y2. Slint text fields (upstream)

### #196 Android: a long press on a field that has no focus yet selects nothing
- Where: Slint's Android backend (`long_press_timeout` only acts on the focused item) and core `TextInput` (on Android it takes focus on release, `items/text.rs` Released branch). Not our code.
- Problem: long-pressing a word in a field you haven't tapped yet selects nothing; you have to tap first. Android's own fields select the word. Found in the review of #195 (2026-10-18).
- Fix: upstream; the long press would need to focus the pressed field first. Report it there.

### #198 Android: Gboard's "line start" key doesn't move the cursor
- Where: Slint's Android backend, not our code. It maps `KEYCODE_MOVE_HOME` to Home (`androidwindowadapter.rs` `map_key_code`), so the cause is unknown.
- Problem: in Gboard's text editing panel, the |< key left the cursor where it was in a Slint text field; the arrow keys and Select worked. Seen once in the material gallery on the API 35 emulator (2026-10-19), while testing Slint PR #14205; not reproduced, not tried in Meditate or on the FP5.
- Fix: first reproduce it (with and without Select), log which key event or `setSelection` arrives, then fix or report upstream.

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
- Where: `meditate-core/Cargo.toml:20` (`ureq = "2"`, built-in roots only), `sync/webdav.rs:176`.
- Idea: also trust the system store or a user-pinned certificate. A new dependency changes Cargo.lock: ask first.

### #190 The day ends at midnight, also for night owls
- Where: every "today" in core (`time::today_local`, goal, streak, heatmap, Log day groups) counts by calendar day.
- Idea: a "day starts at" time (for example 3:00), so a 1 a.m. session counts for the day the user lived. It changes counting in both apps (goal, streak, heatmap, Log groups, synced sessions on another device), probably as a setting. Raised during batch J, 2026-10-13.

## Deferred refactors

Only together with other work in the same area; none has a bug behind it.

- **R12, one blob routine for sounds and guided files** (the runner half moved to core in batch F): about -90 in the orchestrator; the remote paths must stay byte-identical.

## Considered and rejected

So these aren't proposed again:
- Typed serde structs for every sync event payload: the silent defaults are deliberate compatibility tolerance, and there is wire risk.
- A change-counter-driven view refresh: `LocalChanges` is per connection, so sync pulls never bump it.
- A run token on every drop file: only imports needed one (done).
- Number fields that commit on every keystroke: intermediate values go stale or clamp mid-typing.
- Removing the picker's `transient.<ext>` copies: one per extension, overwritten by the next pick, and the guided play-now selection plays the copy in place, so cleanup needs three separate paths for a few MB.
- R13, small duplicates (`hm_mins_key` into `hm_secs_key`, one `session_payload()`): too little gain for touching the sync payload.
- R7(b), a session record instead of loose session state: about -30 lines for about 54 rewritten test chains.
- R15(c), one `SlidePage` frame for 13 pages: every page reaches into its own Flickable, so each needs rework and a retest, for a repeated frame and no bug.
- R15(d), merging `VerticalSpinBox` and `StepperRow`: about -40 lines in the focus and keyboard code the cursor bugs came from.
- R2b, all database writes off the UI thread: about 175 call sites, and no lock wait noticed in practice.
- Closing the file picker before its copy ends (#37): the read grant on the picked file can end with that screen, so every pick path would need the file opened first; a local copy takes about a second.
- Typed `BellSlot` through Slint: large, with no host safety net.
- Moving the whole sync account Save/Test into core: the keychains are per app.
- Moving the Log edit build into core: low value; reconsider when fixing #5/#6.
- Splitting `ui.rs` per screen or the big core files: cosmetic.
- A `Divider` component, a page stack model, caching JNI classes, moving the CAN_DUCK policy to Rust, a typed `SettingKey` enum, a core pending-Undo queue.
- Back on the Done screen discarding the session (decided behaviour).

### #194 Back up everything, not only the Log
- Where: Preferences backup in both apps; the CSV export (`meditate-core/src/data_io.rs`) carries sessions and labels only.
- Problem: without sync, presets, interval bells, Box Breath patterns, vibration patterns, imported sounds and guided files, and every setting live only on the device. A lost or reset phone or laptop loses them; the CSV restores the Log alone.
- Idea: a full backup file (the database, e.g. `VACUUM INTO`, which needs no new dependency, plus the sounds and guided audio) and a restore for it, in both apps. Open questions: what a restore does to the device id, the event log and sync state on a device that syncs, and whether it replaces or merges. Raised by the maintainer, 2026-10-18.
