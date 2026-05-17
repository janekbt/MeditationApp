# Android Port Plan

Status: In progress on `beta`. Workspace restructure, toolchain script, Material
hello-screen, Countdown→Session migration, and the consolidation back onto
`beta` (2026-05-14) are all landed. UI translation work tracked under
"UI Translation Phases" below; platform-edge milestones in their own section
further down. Both lists progress in parallel.

Owner: Janek (solo). No PRs; same `beta`-first discipline as the GTK shell.

## Goal

Ship the meditation app on Android by reusing `meditate-core/` (timer,
db, format, breath, sync — pure Rust, no GTK) under a Slint UI built
with the Material 3 component library. The current GTK4 / libadwaita
shell is the permanent Linux-first shell — it is not slated for
replacement at any point. The Android port is a parallel, lower-priority
target.

## References (consult regularly during implementation)

- Slint Material 3 component docs and API reference: <https://material.slint.dev/getting-started/>
  Re-read before starting each milestone so we use current
  components / props / theming hooks rather than what the model has
  cached.
- Slint Android backend docs: <https://docs.slint.dev/> (search for "Android" — exact path subject to change between versions).
- The default-style consolidation post (Mar 21, 2026) — <https://slint.dev/blog/default-native-style-change> — for the explicit carve-out: Material is the mobile recommendation; Fluent is the desktop default.

## Decisions (settled)

| Decision | Choice | Rationale |
|---|---|---|
| UI framework | Slint + Material 3 components (`material.slint.dev`) | Rust end-to-end, terminal-first build, smallest second-codebase burden. Material 3 components went stable Sept 2025; the Mar 21, 2026 default-style consolidation post (which post-dates Material's stable release) explicitly carves Material out as the mobile recommendation, so the direction is a recent re-affirmation, not stale framing |
| Distribution | F-Droid eventually; no Play Store | Avoids Play Store target-SDK churn and signing-key custody; F-Droid signs with its own key |
| Workspace shape | Top-level becomes a Cargo workspace, GTK shell renamed to `meditate-gtk/` | Clean host/target split; single source of truth for shared deps |
| Branch flow | Long-running `android` off `beta`. Merge back when shippable. | Keeps `beta` releasable; Linux work continues uninterrupted |

## Known caveats accepted

1. **Material You dynamic color** (wallpaper-derived palette) is unlikely to be wired up by the Slint Material crate. We get canonical Material 3 colors, not the user's personalised scheme.
2. **Edge-to-edge insets, IME padding, predictive back gesture.** Slint runs on `NativeActivity`; rough edges expected. Verify on real device early.
3. **Accessibility / TalkBack** is thinner than Compose's. Acceptable for v1.
4. **Slint's Material crate could be deprecated.** Mitigation: pin a specific version; if abandoned the pinned version keeps working at its frozen feature set.

## SDK floor / ceiling

- `minSdkVersion`: 26 (Android 8.0). Covers ~98 % of in-use devices, dodges the SDK-25-and-below paths in `Vibrator` and notification APIs.
- `targetSdkVersion`: 35 (Android 15). F-Droid does not require a current target SDK, but staying current avoids accumulating compat shims later.
- NDK: r27 (matches what `cargo-apk` and `slint-android` test against; pin in setup script).

## Workspace restructure

Top-level `Cargo.toml` becomes a workspace manifest:

```
MeditationApp/
├── Cargo.toml             (workspace root, no [package])
├── meditate-core/         (unchanged — shared logic)
├── meditate-gtk/          (current src/, renamed from `meditate`)
│   ├── Cargo.toml
│   ├── src/
│   ├── data/              (Blueprint UI, .desktop, schemas)
│   └── build.rs
├── meditate-android/      (new — Slint shell, cdylib for cargo-apk)
│   ├── Cargo.toml
│   ├── ui/                (.slint files)
│   ├── src/lib.rs
│   └── android/           (AndroidManifest.xml, res/ if needed)
└── build-aux/             (shared scripts: dev-xbuild, setup-android, cargo-sources regen)
```

The host-only deps (`gtk4`, `libadwaita`, `gstreamer`, `oo7`,
`gettext-rs`) stay in `meditate-gtk/Cargo.toml` and never appear in
the Android target's dep graph. `meditate-core` already has only
portable deps and needs no changes.

`meson.build`, `build-aux/dev-xbuild.sh`, and the Flatpak manifest
update to point at `meditate-gtk/` (one-time path edit). Flathub
build keeps working unchanged otherwise.

## Dev tooling on this Debian laptop

Installed by `build-aux/setup-android.sh` (idempotent, Debian/Ubuntu only):

| Component | Source | Pinned version |
|---|---|---|
| OpenJDK 17 | `apt install openjdk-17-jdk-headless` | distro |
| Android command-line tools | direct download (Google) | latest, pinned by SHA |
| Android SDK platform | `sdkmanager "platforms;android-35"` | 35 |
| Android SDK build-tools | `sdkmanager "build-tools;35.0.0"` | 35.0.0 |
| Android platform-tools (`adb`, `fastboot`) | `sdkmanager "platform-tools"` | latest |
| Android NDK | `sdkmanager "ndk;27.2.12479018"` | r27 |
| Rust targets | `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android` | per current toolchain |
| Gradle wrapper | committed at `meditate-android/android/gradlew` | 8.5 (AGP 7.3, Kotlin 1.7.20) |

**xbuild was removed** (migrated to a hand-maintained Gradle
project — its manifest struct couldn't emit `<receiver>`/custom
`res/`, blocking Phase 8 theming, F-Droid, and the preset
widget). No Android Studio. No emulator image (opt-in via
`--with-emulator`). Build + on-device deploy:

```
. ~/.config/meditate-android/env.sh
cd meditate-android/android
./gradlew :app:assembleDebug          # runs rust-build.sh (cdylib→jniLibs)
adb -s <serial> install -r app/build/outputs/apk/debug/app-debug.apk
adb -s <serial> shell am start -n io.github.janekbt.Meditate/android.app.NativeActivity
```

We still do *not* commit `~/.cargo/config.toml` cross-linker
entries: `meditate-android/android/rust-build.sh` exports the
per-ABI NDK linker/CC/AR for that one `cargo build` invocation
only. We deliberately do **not** use `cargo-ndk` — its 4.x
runner sanitizes the build env so `i-slint-backend-android-
activity`'s `build.rs` fails the SDK lookup ("No Android
platforms found"); plain `cargo build --target` keeps
`ANDROID_HOME` intact.

Env vars (`ANDROID_HOME`, `ANDROID_NDK_ROOT`, `JAVA_HOME`, plus
`PATH` extensions for `sdkmanager`, `adb`, etc.) live in a single
sourced file at `~/.config/meditate-android/env.sh` so the script
owns the file outright and can rewrite it idempotently. A tiny
marker-bracketed block in `~/.bashrc` sources it.

### Setup-script contract

`build-aux/setup-android.sh`:

- Refuses to run on non-Debian/Ubuntu (`/etc/os-release` check) with a clear message.
- Top-of-file `PINNED_VERSIONS` block; one place to bump.
- Each install step is gated on a presence check (`which`, `sdkmanager --list_installed`, `cargo install --list`).
- Writes `~/.profile.d/android-sdk.sh` with `set -u`-safe exports.
- Appends `~/.cargo/config.toml` only if the `[target.aarch64-linux-android]` block is missing (idempotent merge, not blind append).
- Final step: smoke test — `cargo apk --version`, `adb version`, print `ANDROID_NDK_ROOT/source.properties`.
- `--with-emulator` flag installs the system image and creates an AVD.
- Exit non-zero with a useful message on any step failure; do not partially-write state.

### Memory ceiling note

This laptop has 15 GiB RAM and ~1 GiB swap (see
`reference_dev_laptop` memory). A Slint LTO build uses similar memory
to the current GTK Linux build; running both an `aarch64-linux-android`
release build and the GTK Flatpak build in parallel will tip over.
The setup script does not mitigate this — it's a workflow note for
when iterating: pause Nextcloud sync before heavy builds, run one
target at a time.

## Milestones — platform-edge build-out

The systems-side path: toolchain, JNI bridges, services. Orthogonal
to the UI translation phases below; each commit can advance either
axis (or both). Shipping = both axes far enough along that a release
is coherent.

1. ✅ **Workspace restructure** (commit `51e5cda`, replicated on `beta` as `17440ea`).
2. ✅ **`setup-android.sh` lands** (`25c304e`).
3. ✅ **Empty `meditate-android` crate** — Slint Material hello-screen (`fd0993f`).
4. ✅ **Wire `meditate-core` to one screen** — originally `Countdown`, migrated to `Session` in `73c5cc4`. The dead Countdown/CountdownTimer primitives dropped in `61d56be`.
5. ✅ **Mode toggle + Box Breath mode + per-mode stopwatch toggle** — chip group (`54a66c4`), Stopwatch SwitchRow (`46ae266`), Box Breath phase grid (`af3d38d`), Box Breath running visualisation (`da7383f`). State machine flows through `SessionShape::TimerCountdown` / `TimerStopwatch` / `BoxBreathCountdown` / `BoxBreathStopwatch`; per-mode session length backed by `breathing_session_secs` (Box Breath, DB-persisted) and an in-memory Timer cell.
6. ✅ **DB persistence** — `Database::open` + `seed_all_non_audio` at `android_main` entry (`5b74887` + `bc1baab`); per-session insert on Save (`a741d2e`); settings persistence for `keep_screen_awake_*`, `*_signal_mode`, `label_*`, `breathing_*`, `*_stopwatch_active` (`45171ab`, `1625726`, `bc1baab`, `af3d38d`, `c31e771`); crash-recovery snapshot heartbeat + startup `finalize_session_in_progress` (`f12eea4`). Log view + edit-session dialog + Undo toast surface are UI work (Phase 3).
7. ✅ **Bells.** Audio via Android `MediaPlayer` JNI — `MeditateAudio.kt` + `audio.rs` app-classloader bridge; bundled OGGs `include_bytes!`'d + extracted to `<data_dir>/sounds` and seeded into `bell_sounds` (`3d04ea6`, `fdd004e`); preview returns clip duration for pill auto-revert (`46b2485`).
8. ✅ **Haptics.** Android `VibratorManager`/`Vibrator` JNI — `MeditateHaptics.kt` `createWaveform` + `cancel` with `USAGE_ALARM`; `haptics.rs` marshals the `meditate_core::vibration::build_master_envelope` `(amp, ms)` sequence to `long[]`/`int[]` (`3dd000f`).
9. ✅ **Foreground service + notification** — Kotlin `MediaSessionService` started via JNI bridge with classloader walk (`76dbe6f`), MediaStyle pin so the notification lives in the shade's Media-controls section (`ae6104f`). Phase-5 audio playback plugs into the same notification when it lands.
10. **Keychain.** Android `KeyStore` JNI for the Nextcloud app-password. `oo7` is host-only. Drives UI phase 7's password row.
11. **Sync.** `meditate-core::sync` already abstracts the HTTP layer (`ureq`). Plug in the JNI keychain and run. Pairs with UI phase 7.
12. **Polish: edge-to-edge, predictive back, IME insets, theming.** Predictive-back partly done (root `FocusScope` routes `Key.Back` through a Rust handler that closes the labels chooser → discards Done → swallows back during Running, commit `9e95a5d`). Edge-to-edge / IME insets / theming still pending.
13. **F-Droid metadata + reproducible build.** Manifest, fastlane structure, `metadata/io.github.janekbt.Meditate.yml` for fdroiddata. Lands after every UI phase is at least partly usable.
14. ✅ **Home-screen preset widget** (Android-only — no GTK mirror). RemoteViews collection over a Rust-written JSON projection of starred presets across Timer + Box-Breath; `widget.rs` + `MeditateWidget*.kt`. Tap deep-links into an auto-started session via a broadcast → drop-file → `try_widget_deep_link` channel, cold and warm (NativeActivity forwards no `onNewIntent`). W-1/2/3 `6a2d66c`, W-4 `69be0ad`. Unblocked by the xbuild→Gradle migration (`<receiver>` + `res/`).

## UI Translation Phases

The screen-by-screen path: each phase ports one or two GTK screens to
Slint + Material 3 end-to-end. Slicing is by screen, not by feature,
so the app is always shippable mid-port (just with fewer screens).
Each phase ends with on-device verification on the Fairphone 5 + the
GTK shell still passing `cargo test --workspace` + `cargo clippy
--workspace --all-targets -- -D warnings`.

**Principle.** Mirror GTK behavior per the saved feedback memory:
before each phase, re-read the GTK shell's file for that screen;
copy layout, copy text, and copy behaviour as closely as Material 3
allows. Deviate only where MD3 demands it.

### Phase 1 — Setup view + Timer running view ✅

GTK reference: `meditate-gtk/data/ui/timer_view.blp` (setup) + the
running-page builder in `meditate-gtk/src/window/imp.rs`.

Landed:
- Duration picker (tap-to-open H:M dialog, default 0h 10m) — mirrors
  the GTK `Adw.ActionRow` + custom time dialog (`f8035c5`,
  `fc71e80`).
- Hero readout via `Session::display_secs` for Running / Paused /
  Finished + `format_hhmm` for Idle (`a741d2e` and earlier).
- Streak chip slot above the hero — empty placeholder property
  (`streak-text`) so the layout doesn't reshape when phase 3 fills
  it (`54a66c4`).
- Pause / Stop affordances on the running page (the action button
  morphs Start / Pause / Resume via `primary_label`; Stop is the
  destructive sibling) (`a741d2e` and earlier).
- Foreground-service scaffolding lands here cross-cutting from
  systems milestone 9; the service survives screen-off + posts a
  Media-style pinned notification (`76dbe6f`, `ae6104f`).
- Setup → Running slide-over (only Running animates, Setup stays
  put underneath, mirroring `AdwNavigationView.push` semantics)
  (`c85a8c2`).

Deferred:
- Add-time / Finish-overtime affordances when in Overtime. Auto-end
  on target-reached works; user-driven extension does not.

Verification: 1-minute countdown round-trips Start → Pause → Resume
→ Stop with timings matching the GTK shell to ±100 ms; screen-off
during a session doesn't kill it.

### Phase 2 — Mode toggle + Box Breath mode ✅

GTK reference: the three-mode segmented control at the top of
`timer_view.blp`, plus the Box-Breath phase visualization built in
`meditate-gtk/src/timer/imp.rs`.

Landed:
- Mode chip group (Timer / Guided / Box Breath) below the hero,
  mirroring `Adw.ToggleGroup mode_toggle_group` toggle order
  (`54a66c4`). Guided is a placeholder — Start disabled, body
  shows a "lands in phase 5" note.
- Cues row with compact three-option toggle (Sound / Vibration /
  Both), persisting per-mode via
  `signal_mode_key_for_mode` (`1625726`). Audio + haptic engines
  themselves are platform-edge phase 5.
- Stopwatch Mode SwitchRow — flips
  `TimerCountdown`/`BoxBreathCountdown` ↔ `TimerStopwatch`/
  `BoxBreathStopwatch`, greys the Duration row, bypasses the
  non-zero duration check on Start (`46ae266`).
- Keep Screen Awake SwitchRow, per-mode persistence via
  `keep_screen_awake_key_for_mode` (`45171ab`). Real WakeLock
  acquisition is platform-edge phase 8.
- Box Breath phase grid in Setup: 2×2 PhaseTile (Inhale /
  Hold (full) / Exhale / Hold (empty)) with `−  Ns  +` steppers,
  per-phase min-policy from `BreathPattern::phase_min_secs` (1
  for inhale/exhale, 0 for holds), max 20 (`PHASE_MAX_SECS`).
  Per-phase + session-length values persisted via the same
  settings keys the GTK shell uses (`breathing_in`, …,
  `breathing_session_secs`) (`af3d38d`).
- Box Breath running visualisation: 220×220 container with a
  196×196 rounded square frame + animated dot + centred phase
  label + per-phase remaining seconds + "Box Breathing" eyebrow
  + counter strip. Mirrors `push_breathing_running_page`. Dot
  follows a rounded-corner trajectory (Android-only deviation
  from core's `Phase::perimeter_point`, which keeps sharp 90°
  turns for GTK) via the shell-side `rounded_perimeter_point`
  helper. `Session::box_breath_phase_info` drives the per-tick
  refresh; `bb_target_secs` shell-cell carries the cycle-aligned
  target for the `box_breath_counter_label` "elapsed / target"
  branch (`da7383f`).

Verification: Box-Breath cycle alignment matches GTK end-of-cycle
timing; stopwatch toggle on Box Breath produces a count-up readout.

### Phase 3 — Persistence (Log view + session-save flow)

Systems milestone 6 lands first / alongside. GTK reference:
`meditate-gtk/src/log/imp.rs`. The undo-toast plumbing already has
its typed `Announcement::SessionDeleted` variant in core (commit
`9dd54a7`).

Landed early (session-save half):
- Done screen with elapsed readout + auto-grow note field + Save /
  Discard (`0ac7501`). Slide-out animation on Save / Discard
  (Android-only deviation — GTK uses instant view-stack swap)
  (`95a7a01`).
- Stop / Finish persists via `Database::insert_session` with
  start-unix, elapsed, mode, note, and resolved label_id
  (`a741d2e`).
- Label expander on Setup + chooser overlay + create / rename /
  delete dialogs, per-mode UUID setting persistence
  (`bc1baab`, `9e95a5d`, `3810ba8`, `8bbc73d`, `ef9047f`).
- Label expander on Done with chooser routing (`chooser-target`
  flag) + persist-back-to-mode on Save via `resolve_persist_action`
  (`68c5ba6`).
- Crash-recovery: 60-second snapshot heartbeat tied to session
  start / end + startup `finalize_session_in_progress` (`f12eea4`).
  Undo toast for the recovered row deferred until the Snackbar
  surface lands.
- Log screen (L-1/L-2): session cards grouped by day, read-only,
  with NavigationBar + pagination ("Load more"), colour-matched
  label stripe (`3cd1c42` + predecessors).
- Delete + Undo snackbar (L-3): per-card trash button, coalescing
  "N sessions deleted · Undo" Material Snackbar with 5 s
  auto-commit timer (`acef04d`). This also landed the Snackbar
  surface itself.
- Edit-session overlay (L-4a–d): Note (`33ffd36`), Duration via
  the Setup-screen popup (`bb0ce2a`), Start time via Material
  DatePickerPopup + TimePickerPopup (`0543d52`), Label group
  reusing the labels chooser (`99ad8e8`).
- Log filters (L-5a–c): funnel button (Log-only) + filter sheet,
  Has-Notes switch, Label DropDownMenu, "No Matching Sessions"
  empty-state variant (`fc630ab`, `bcdd933`, `954f74a`).
- Crash-recovery Undo (L-6): rescued session surfaced in the
  shared Snackbar ("Recovered N min session" + Undo, 8 s),
  Undo → `delete_session_by_uuid` (`b81cd97`).
- Sync-status indicator surface: trailing AppBar
  spinner/warning/check driven by
  `meditate_core::sync::indicator::state_from_db`. Renders
  Hidden on Android until the Phase 7 sync loop exists; tap is
  a no-op placeholder until `action_for` targets land.

**Phase 3 complete.** Cross-device sync round-trip
verification is deferred to Phase 7 (no Android sync loop
yet) — the persistence + Log surfaces it depends on are all
in place.

### Phase 4 — Stats view

GTK reference: `meditate-gtk/src/stats/imp.rs`. Mostly rendering —
every metric has a `*_from_db` reader in `meditate-core`; the
custom-painted bits (chart, heatmap) became declarative Slint
(layout-positioned bars / `Path` line / Rectangle grid) rather
than Cairo.

Landed (S-series, 3rd nav tab — order Timer/Stats/Log to match
GTK's ViewSwitcher):
- S-1: scaffold + Mini-stats row (Streak / Total / Sessions)
  (`e10718c`).
- S-2: weekly-goal hero — `CircularProgressIndicator` ring +
  progress / status labels (`0a8ffd1`).
- S-3: insights boxed list — all 9 `InsightKey` variants, GTK
  glyphs swapped for bundled SVGs (Android-font tofu) (`78441e8`).
- S-4: by-label breakdown, hidden when no labelled sessions
  (`6e1a4c2`).
- S-5a: period toggle (Week/Month/3M/Year) + declarative bar
  chart (`88991dd`).
- S-5b: Bars/Line toggle — line + area via Rust-built SVG
  `Path` commands (`e7696ae`).
- S-6: 13-week contribution heatmap (`contrib::build_grid` →
  13×7 Rectangle grid, primary-alpha level ramp) + range
  caption + legend.
- Cross-shell fix: chart axis caption tracks the aggregation
  tier via new `date_math::chart_unit_for_days` (`a418e89`).

- Log "Add Session" button: AppBar Add button (Log-only)
  opens the Edit-Session overlay in create mode (None ⇒
  `insert_session`, Timer-mode default); same overlay surface
  as edit. Two overlay bugs fixed alongside: Material
  DatePickerPopup emits the stale input date on OK (read
  `get_current_date()` instead), and the label sub-row must
  use the `ExpanderRow` component, not `if`-gated children
  through `BoxedList`'s `@children` (Slint won't reactively
  re-render those).

**Phase 4 complete** — every Stats section + the Log Add path
shipped.

Verification: numbers match the GTK shell on the same DB; heatmap
matches visually for the same period range.

### Phase 5 — Bells + vibration patterns ✅

Two-layer phase. Systems milestones 7 + 8 (audio + haptics JNI)
landed in parallel with the UI work. GTK reference: bell-sound chooser
(`meditate-gtk/src/sounds.rs`), vibration-pattern chooser
(`meditate-gtk/src/vibrations.rs`). The `PreviewToggle` machinery is
already typed in core (`meditate_core::sound::PreviewToggle`).

Landed:
- ✅ Bell-sound chooser with per-row Play/Stop preview via
  `PreviewToggle` + audio JNI (B-4, `46b2485`; back-gesture stop
  `e8e4427`).
- ✅ Vibration-pattern chooser, same shape, haptics JNI (B-2b,
  `eb3a0b8`).
- ✅ Starting / Interval / End bell rows in Setup view, incl.
  Sound/Vibration/Both Type toggle + Pattern row (B-2b) and the
  GTK-row interval-bell editor (B-2c, `fbdfd71`).
- ✅ Box-Breath per-phase cue config (master toggle + 4 phase
  expanders, B-7, `0ff829e`). Phase Sound chooser filtered to
  `BellSoundCategory::BoxBreath` (GTK parity — empty until
  voice-cue audio is sourced; documented TODO in both shells).
- ✅ Bells + patterns fire during a real session — effect-driven
  (`AppState`→`dispatch_effects`, mirrors GTK's
  `dispatch_session_effects`) with proper Overtime (B-6,
  `f7c5557`); Stop/Pause silent; duration persists (`4876de0`).

**Phase 5 complete** — every bell/pattern/cue surface is
configurable and fires correctly on the FP5. Only open follow-up
is bundling soft Box-Breath voice/marker audio (shared TODO with
the GTK shell, not an Android gap).

Verification: starting a Timer countdown with all three bells +
vibration enabled fires all three sounds + vibration at the right
phase boundaries.

### Phase 6 — Editors + Presets ✅

GTK reference: vibration pattern editor
(`meditate-gtk/src/vibration_editor.rs`), preset chooser /
manage page (`meditate-gtk/src/presets.rs`), label chooser
(`meditate-gtk/src/labels.rs`).

Landed:
- ✅ Preset chooser + Save / Override / Manage flow — the full
  GTK-shaped shared overlay (P-1..P-5, `a01980c`, `15f8a72`,
  `972cf4e`, `565b283`): starred-preset row list in Setup,
  apply with snapshot-Undo, Save Settings → create dialog,
  Manage (star / rename / delete + Undo), all through the
  single-slot snackbar discriminators. Decisions stay in core
  (`preset_config::{snapshot, apply}`).
- ✅ Label chooser screen + per-row rename / delete with Undo —
  landed earlier in Phase 3 (`bc1baab`, `9e95a5d`, `3810ba8`)
  + the declare-overlays-last z-order fix (`0ff829e`); GTK
  parity verified during P-4/P-5.
- ✅ Vibration pattern editor (V-1..V-5, `713056a`, `59c7500`,
  `13deb30`). Interactive curve: Slint `Path` rebuilt in plot
  pixel space (viewbox pinned so curve + draggable handle dots
  share one coord space), `TouchArea` drag → core pick / snap;
  Bar = one solid stepped polygon, Line = polyline + soft fill;
  GTK-shaped axis labels (Y %, X seconds overlap-thinned by
  core `select_xlabel_indices`). Save/validate/preview through
  the chooser Create/Edit entry; decisions in
  `meditate_core::vibration` (geometry helpers, `binarize`),
  strict TDD. Graceful degradation: vibrators with no
  amplitude control (e.g. FP5) edit Bar-only binary patterns
  (the platform can't render anything between 0/100% anyway;
  PWM duty-cycle emulation was tried + reverted as it felt
  bad). On-device confirmed (FP5).

**Phase 6 complete.** Presets, label management, and the
vibration-pattern editor are all done and on-device confirmed
(FP5).

Verification: a custom pattern authored on the phone syncs to the
laptop and renders identically in the GTK shell's editor.

### Phase 6.5 — Guided meditation mode

Not in the original phase list (Guided was placeheld "lands with
the audio engine"). `meditate-core` already supports it fully —
`SessionMode::Guided`, `SessionShape::Guided { duration_secs,
count_up_display }`, the `guided_files` table + ops, per-mode
settings keys, the hero-label helper, and `Session`
pause/resume/overtime/stop. **No core changes** — all Android
shell. Mirror GTK (`meditate-gtk/src/guided.rs`).

Decisions: import transcodes to **Ogg/Opus** (Android has no
native Vorbis encoder; Opus-in-Ogg is native via
MediaCodec+MediaMuxer and equally sync-safe — the Librem side's
gstreamer `decodebin` plays it; only the codec inside the `.ogg`
differs from GTK's Vorbis, invisible to the user). The SAF
picker can't return through `NativeActivity.onActivityResult`,
so a tiny Kotlin helper Activity runs `ACTION_OPEN_DOCUMENT`,
copies the pick into app storage, and hands path+duration to
Rust via a drop-file (same pattern as the widget launch).

- ✅ **GM MVP** (transient Open-File path), GM-1..GM-4: Guided
  Setup row + tap-to-pick; SAF picker via a Kotlin shim Activity
  (NativeActivity can't `onActivityResult` → drop-file → Rust
  tick poll, like the widget launch); `MeditateGuided`
  MediaPlayer plays the picked file (USAGE_MEDIA/SPEECH),
  pause/resume/stop follow the session, onCompletion/onError →
  `guided_eos` drop-file → tick forces `AppState::enter_overtime`
  → end bell (robust to probe-vs-real mismatch);
  `SessionShape::Guided { duration_secs, count_up_display }`
  built from the pick; persisted via the existing
  `finalize_session` path with `guided_file_uuid = None`. No
  core changes; new core-adapter `enter_overtime` + Guided
  lifecycle tests (strict TDD). On-device confirmed (FP5).
- ⬜ **GM follow-up**: Import (Ogg/Opus transcode + auto-star),
  starred-files list in Setup, Manage-files chooser
  (star/rename/delete/preview) — the full `guided.rs` parity.

### Phase 7 — Sync + Preferences

Systems milestones 10 + 11 (keychain JNI + sync runner) land first.
GTK reference: `meditate-gtk/src/preferences.rs`,
`meditate-gtk/src/recovery_dialog.rs`.

To land:
- Preferences screen (Material 3 settings list pattern).
- Nextcloud URL / username / password rows wired through the
  Android `KeyStore`.
- Account-test button rendering `SyncSettingsError` variants via the
  shell-side gettext renderer.
- Recovery dialog (push-local / wipe-local) — the `prepare_*_recovery`
  helpers are pub on `meditate_core::sync::settings`.
- Sync status indicator becomes interactive.

Verification: real Nextcloud sync round-trip from the phone with
the laptop GTK shell as a peer; the account-test toasts match the
copy in the GTK shell.

### Phase 8 — Polish

Mostly fit-and-finish; corresponds to systems milestone 12.

- Edge-to-edge layout (no system bars eating UI).
- Predictive-back gesture works on every screen.
- IME insets (text fields stay visible when the keyboard appears).
- Material 3 light/dark theming — no Material You per the accepted
  caveat.
- TalkBack labels + roles on every interactive widget.
- i18n: decision between `.po` parsed in Rust vs. Android string
  resources lands here.

Verification: every screen passes a TalkBack pass; rotate / multi-
window / dark mode / keyboard interactions all behave; F-Droid
build reproducibility check.

## Cross-cutting concerns

These don't live in any single phase but get touched throughout:

- **Navigation.** Phone-first single-pane stack. Slint's
  `NavigationStack` mirrors `AdwNavigationView`'s shape; back gesture
  pops, the title bar's leading icon is the back arrow. The
  desktop-adaptive sidebar concerns of libadwaita don't apply on
  Android.
- **Toast / SR-announcement plumbing.** Reuse
  `meditate_core::announcement::Announcement` (six variants today);
  render via Material 3 Snackbar. The same enum drives the GTK
  shell's `crate::announcement::title()` helper.
- **Tick loop.** Slint timer at ~200 ms feeds `Session::tick(now)`;
  the same loop dispatches `Effect`s. Hooked up from phase 1; later
  phases add Effect handlers (bell-fire, vibration-fire, etc.).
- **State adapter.** `meditate-android/src/app.rs` keeps the AppState
  pattern that the Session migration just landed. Every later phase
  extends AppState with the next screen's state, not via a fresh
  module — unit-testable below the Slint runtime, same shape that
  has 36 tests today.

## Discipline rules

Borrowed from the GTK shell's process; non-negotiable:

- On-device verification on the Fairphone 5 before commit of any
  user-facing change (the "test on device before claiming fix works"
  rule applies to both shells now).
- Core changes touch both shells in the same commit; no separate-PR
  dance. The branch consolidation makes this trivial.
- The GTK shell stays green through every phase: `cargo test
  --workspace` passes, `cargo clippy --workspace --all-targets --
  -D warnings` passes.
- Strict TDD on anything below the Slint runtime — the adapter layer
  in `meditate-android/src/app.rs` style.
- No `#[allow(dead_code)]` to silence warnings — fix the cause (the
  recent Countdown removal is the template).

## Platform-edge replacement matrix

| Concern | Linux (current) | Android (new) | Where the JNI lives |
|---|---|---|---|
| UI toolkit | GTK4 + libadwaita | Slint + Material 3 | n/a (Slint is Rust) |
| Audio | `gstreamer-rs` | `MediaPlayer` or `oboe-rs` | `meditate-android/src/audio.rs` |
| Haptics | `libfeedback` D-Bus | `Vibrator` / `VibratorManager` | `meditate-android/src/haptics.rs` |
| Keychain | `oo7` (Secret Service) | Android `KeyStore` | `meditate-android/src/keychain.rs` |
| i18n | `gettext-rs` | `.po` parsed in Rust, OR Android string resources | TBD milestone 12 |
| DB | `rusqlite` bundled | `rusqlite` bundled (works as-is) | nothing |
| Notifications | none (foreground app) | `NotificationManager` + foreground service | `meditate-android/src/service.rs` |
| Suspend-resilient timing | `clock_gettime(CLOCK_BOOTTIME)` via libc shim | `Instant::now()` (Rust 1.79+ uses `CLOCK_BOOTTIME` natively on Android) | `meditate-core/src/time.rs` cfg-gates the shim |

## Open questions (pre-milestone-1)

- `meditate-core` currently isn't `no_std`; not relevant on Android (linux-android target has full std), but worth noting for any future embedded port.
- The current `cargo-sources.json` regen flow exists for Flatpak's offline build; F-Droid's reproducible-build flow needs its own audit when we get to milestone 13. Out of scope for now.
