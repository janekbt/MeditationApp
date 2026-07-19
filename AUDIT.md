# Publish-readiness audit — 2026-07-19

Working document. Severity: **Blocker** (breaks on real phones /
legal), **Important** (degraded UX, latent bug, contributor
confusion), **Polish**. Status: `open` until fixed/waived.

## Phase 0 — Inventory

- meditate-core ~31.6k LOC, meditate-gtk ~18.4k, meditate-android
  ~13.7k Rust + ui/main.slint 7.6k + ~9.7k Kotlin (14 files).
- Android: minSdk 26, target/compileSdk 34, arm64-v8a only,
  versionName 0.1.0 (GTK scheme: 26.4.4), hand-written manifest,
  zero appcompat/material Gradle deps.
- 14 root markdown docs, several stale (see Phase C).
- No LICENSE file at repo root (verify in Phase D).
- rust-build.sh sources ~/.config/meditate-android/env.sh —
  outside the repo, undocumented (contributor setup gap).

## Phase A — Portability

### A1 Android API levels (Kotlin + manifest reviewed in full)

Overall: the Kotlin layer is well-guarded — VibratorManager (S+),
VibrationAttributes (T+), WindowInsets.Type (R+), PendingIntent
mutability flags, FLAG_IMMUTABLE, startForeground timing, GCM key
handling are all correctly version-split. Verified findings:

- [x] **A1-1 Important (FIXED: partial wake lock in MeditateSessionService, WAKE_LOCK permission)** — Tick/bell timing under Doze.
  `src/lib.rs:426` TICK=200ms via `slint::Timer` (UI thread); no
  wake lock, no AlarmManager backstop. The mediaPlayback FGS keeps
  the process alive but not the CPU awake: on phones that enter
  Doze/deep suspend during a long screen-off sit (esp. aggressive
  OEMs: Samsung/Xiaomi), interval/end bells fire LATE (on wake).
  Masked on FP5 by keep-screen-awake. Fix: partial wake lock held
  only while a session is Active (WAKE_LOCK permission; battery
  cost trivial for meditation durations), or exact-alarm backstop.
- [x] **A1-2 Important (FIXED: cutout safe-insets unioned on API 28-29)** — Pre-API-30 insets miss the display
  cutout. `kotlin/MeditateInsets.kt` R+ path ORs in
  `displayCutout()`; the 26..29 fallback uses
  `systemWindowInset*` only → content can sit under a notch on
  Android 9/10 devices. Fix: also consult
  `wi.displayCutout?.safeInset*` on P..Q.
- [x] **A1-3 Important (FIXED: failure now raises the translated import-failed snackbar; API floor documented in BUILDING.md — specific cause stays in Diagnostics)** — Guided import of non-wav/ogg needs
  API 29 (`MUXER_OUTPUT_OGG`, Opus encoder). Correctly guarded
  (`MeditateGuidedImport.kt` throws "Import needs Android 10+")
  but the message reaches the UI raw and untranslated, and the
  limitation is undocumented. Fix: translate + document; consider
  minSdk 29 (simplifies support matrix; Android 8/9 share <3%).
- [x] **A1-4 Polish (FIXED: strings.xml + values-de, widget title + manifest label included)** — Notification strings hardcoded English
  ("Meditate", "Session in progress", channel "Session") in
  MeditateSessionService.kt, not in res/values strings.xml (ties
  into the known widget-strings i18n backlog).
- [x] **A1-5 Polish (FIXED: Light day theme + values-night dark)** — No values-night theme; Theme.Meditate
  parents `Theme.Material.NoActionBar` (dark) → light-mode users
  get a dark cold-start flash.
- [ ] **A1-6 Polish** — `MeditateAudio.play` runs synchronous
  `MediaPlayer.prepare()` on the caller (UI) thread.
- [ ] **A1-7 Polish** — Failed SAF pick (null stream / copy error)
  is silent: no drop-file → no UI feedback (verified no stuck
  state; row just stays unset).
- [ ] **A1-8 Info** — targetSdk 34: fine today; 35 will force
  edge-to-edge (insets plumbing already exists). Widget deep-link
  BAL relies on the widget-tap grace window — standard pattern,
  may be restricted further by some OEMs.

### A2 Screen/layout (main.slint sweep)

- [x] **A2-1 Important (FIXED: all 16 dialogs clamped to min(Xpx, root.width - 32px))** — Fixed dialog widths overflow small
  screens. ~12 dialogs at `width: 320px` (main.slint:4872,4951,
  6375,6433,6491,6555,6608,6666,6720,7442,7559), 280px (6781,
  7615), 340px (7682,7738,7798). At ≤340dp logical width (320dp
  phones, "display size" zoom) the 340px dialogs clip and 320px
  ones touch the edges. Fix: `width: min(Xpx, root.width - 32px)`.
- [x] **A2-2 Important (PARTIAL: adaptive spacing + hero shrink below 560px height; 220px square rescale needs coordinated Rust perimeter-math change — follow-up)** — Landscape: running view (hero 80px +
  buttons) and especially Box Breath (eyebrow+counter + 220px
  square + 56px buttons + spacing ≈ 380px+) overflow vertically on
  <400dp-height screens; no adaptive layout, and rotation is
  enabled (configChanges). Options: portrait-lock the activity, or
  scale the square/hero from available height.
- [ ] **A2-3 Important (a11y)** — OS font-size setting ignored:
  slint px has no sp mapping, so accessibility font scaling does
  nothing. Slint-level limitation — document as a known deferral
  (with TalkBack) and track upstream.
- [ ] **A2-4 Polish** — CuesRow (title + 3-segment toggle) rows are
  tight at 320-360dp width with German labels; spot-check and
  allow the toggle to wrap/shrink if needed.
- Clean: insets wiring (dp-converted, re-queried on resize), page
  scrolling (Flickable-wrapped), hero/box-breath fit portrait
  360dp, text rows use elide/wrap where checked.

### A3 Hardware & lifecycle

- [x] **A3-1 Important (FIXED: AudioFocusRequest on guided start; loss routes through the session-pause transition via guided_focus_loss drop-file; no auto-resume)** — Guided playback ignores audio focus:
  `MeditateGuided.kt` (USAGE_MEDIA) never calls
  requestAudioFocus, so an incoming call / another media app
  plays over the guided track instead of pausing it. Fix:
  AudioFocusRequest (GAIN on start, pause on LOSS/TRANSIENT,
  resume on regain). Bell player (USAGE_ALARM) is exempt by
  design — correct as-is.
- [x] **A3-2 Important (FIXED: rust-build.sh takes ABIS list, arm64 default, x86_64/armeabi-v7a supported; documented)** — arm64-v8a only (rust-build.sh):
  contributors cannot run an x86_64 emulator build, and 32-bit-
  only devices are excluded. Fix: parameterize rust-build.sh over
  ABIs; add x86_64 at least for debug.
- [ ] **A3-3 Polish** — Activity-recreate leaks: build_ui Box::leaks
  slint Timers per android_main run; prior run's 200ms tick keeps
  firing (weak-upgrade no-ops). Benign at real-world recreate
  rates; note in ARCHITECTURE.
- Clean: vibrator-absence handled (hasVibrator + null service),
  WAL dual-connection design, crash-recovery covers process death
  (START_NOT_STICKY + snapshot), keep-awake auto-releases via
  window flag, widget/service exception-hardened.

## Phase B — Correctness

Panic sweep: android shell has 4 shipping unwraps, all fatal-at-
boot by design (MainWindow::new, slint init, ui.run) — acceptable.
Core's `.expect()` family guards enum parses behind SQLite CHECK
constraints — unreachable without a hand-corrupted DB. Sync pull
paths build local filenames from validated DB uuids (target_id
validator rejects `/`, `\`, NUL) + a closed extension set — no
path traversal from a hostile server. data_io/date_math are
panic-clean pre-tests.

- [x] **B-1 Important (FIXED: documented in EVENTS.md "Version skew")** — Version-skew forward-compat: an
  event authored by a NEWER app version whose enum value (mode,
  category, kind) fails an older version's CHECK constraint turns
  into a sync error on the old device (not a crash, not data
  loss — but sync wedges until the old device updates). Document
  in EVENTS.md as the intended failure mode.
- [x] **B-2 Polish (FIXED: sanitize_transport_msg strips URL tokens)** — WebDavError::Network wraps the ureq error
  Display, which can embed the full request URL (server hostname)
  into the user-shareable diagnostics log. Sanitize to host-less
  messages.

## Phase C — Readability & docs

- [x] **C-1 Important (FIXED: README — daily goals, Android section, 3-crate map, meson paths)** — README.md: still "weekly-goal stats"
  (lines 5, 26 — model is daily since 2026-07-17); zero mention of
  the Android app; no Android build instructions.
- [x] **C-2 Important (FIXED: ARCHITECTURE.md — crate map, compaction, Android shell patterns section)** — ARCHITECTURE.md: describes a two-crate
  era ("top-level `src/`", "meditate-android on the `android`
  branch") — reality is a three-crate workspace on beta with a
  Gradle Android pipeline. Needs a rewrite: crate map, decisions-
  in-core rule, event-sourcing + sync + compaction, JNI app-
  classloader pattern, build pipeline.
- [x] **C-3 Important (FIXED: status banner with current-state summary)** — Nextcloud-Sync.md documents the
  superseded one-file-per-event design (now bulk batches +
  compaction manifest). Rewrite or fold into ARCHITECTURE.md.
- [x] **C-4 Polish (FIXED)** — build-aux/setup-android.sh header still
  says it exists "so `x run` (xbuild) can build" — xbuild is gone;
  it now feeds the Gradle pipeline via env.sh.
- [x] **C-5 Polish (FIXED)** — Stale "weekly-goal" doc comments:
  meditate-gtk/src/stats/imp.rs:41, meditate-gtk/src/format.rs:55.
- [x] **C-6 Polish (FIXED)** — VIBRATION_ARCHITECTURE.md:292 references
  /home/janek/Downloads/… mockup path.
- [ ] **C-7 (decision)** — meditate-android/src/lib.rs ~11k lines,
  ui/main.slint ~7.6k: module split proposed as a separate
  mechanical pass (execute if time allows today, else next
  session).

## Phase D — Publishing hygiene

- [x] **D-1 Blocker (FIXED: COPYING (GPL-3.0-or-later) + license fields in all three Cargo.tomls)** — No LICENSE file at repo root and no
  `license =` field in any Cargo.toml. Metainfo declares
  GPL-3.0-or-later. Fix: add GPL-3.0-or-later LICENSE + Cargo
  fields (vendored material-1.0 is MIT with LICENSE.md in-tree —
  compatible; keep. Slint used under its GPLv3 option —
  compatible; note in README).
- [x] **D-2 Polish (FIXED: DejaVu attribution at the import site)** — mask-dot.ttf is a DejaVu subset; add the
  Bitstream Vera/DejaVu attribution note next to the asset.
- **Personal data: clean.** Full-history scan (933 commits): no
  IPs, no e-mail/hostname, no keys. Only hit: the C-6 home path.
  "FP5" device references in comments are legitimate technical
  context. Release signing uses the universal Android debug
  keystore (public constants — not a secret; F-Droid re-signs).
- **Tracked artifacts: clean.** 335 tracked files; build dirs all
  ignored; gradle wrapper committed (Gradle 8.5).
- [x] **D-3 Important (FIXED: BUILDING.md "Android" section replaces the stale xbuild one; setup-android.sh → env.sh chain documented)** — Contributor setup: rust-build.sh hard-
  depends on ~/.config/meditate-android/env.sh (generated by
  build-aux/setup-android.sh) but no doc connects these dots; no
  rust-toolchain.toml. Fix: Android build section in README/
  BUILDING.md + optional env autodetect fallback.
- [ ] **D-4 Polish** — AGP 7.3.0 + Gradle 8.5 + compileSdk 34 is
  an untested-by-Google combo (works, but surprises contributors);
  kotlin plugin 1.7.20 vs kotlinc 1.9.25 in env.sh. Align/document.
- [ ] **D-5 Info** — Version scheme: android versionName 0.1.0 vs
  GTK 26.4.4 — unify before first tagged release.
- [ ] **D-6 Info** — F-Droid gaps: fastlane metadata dir absent;
  reproducible-build story for the cargo step; debug-keystore
  release signing is fine (F-Droid re-signs).

## Phase G — GTK quick pass

Clean: no hardcoded paths/IPs; recovery-dialog copy still accurate
post-compaction (manifest suppresses false positives in core, so
the dialog only fires on genuine wipes). Only C-5 doc comments.

## Pass 2 — 2026-07-19 (evening)

Re-audit of ground pass 1 didn't reach: full snackbar-discipline
check, the small shell modules, JNI exception coverage, slint
z-order, packaging semantics, adversarial review of pass 1's own
fixes.

- [x] **P2-1 Important (FIXED: onTaskRemoved stops the service)** —
  Swiping the app away mid-session left the FGS + (since pass 1)
  the wake lock pinned for up to 4 h with nothing ticking. Now the
  service stops itself; crash-recovery resurfaces the session on
  next launch (same contract as an OOM kill).
- [x] **P2-2 Important (FIXED)** — Six i18n stragglers missed by
  the Tr migration: "All labels" (filter sheet), "New Pattern" /
  "Edit Pattern" (vibration editor title), "Add Session" / "Edit
  Session" (edit dialog title), "Session deleted"/"{n} sessions
  deleted" (log-feed snackbar, had a stale "i18n isn't wired up"
  comment). All routed through Tr + de.po entries.
- [x] **P2-3 Important (FIXED: android:allowBackup="false")** —
  Backup was default-enabled: the session DB (personal notes)
  rode Google/D2D backup transports, and a restored Keystore-
  encrypted sync secret can't decrypt anyway. Nextcloud sync +
  CSV export are the deliberate backup story.
- [ ] **P2-4 Important (design decision)** — CSV re-import
  duplicates sessions: `insert_sessions_with_labels` mints fresh
  uuids and nothing dedupes on (start, duration), so importing
  your own backup into a non-empty DB doubles the log. Shared
  core behaviour (GTK identical). Options: dedupe heuristic on
  exact (start_iso, duration) match, or an "importing N
  duplicates" warning. Cross-shell change — owner's call.
- Verified clean: snackbar raise discipline at all 12 sites (the
  one site without commit_pending_deletes is the delete flow
  itself — correct by design); overlay z-order (all dialogs
  declared after AppBar/NavBar, snackbar relocated with note);
  JNI exception_check present in every bridge module;
  diagnostics log trims to 2000 lines on open; 6 DB indexes
  cover the hot queries; sync_runner clean; widget projection
  write is tmp+rename atomic; 3 TODOs in the whole workspace
  (all the known voice-cues item); pass 1's focus-loss drop-file
  is self-cleaning (consumed every tick regardless of state).
