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
| `xbuild` (`x` CLI) | `cargo install --git https://github.com/rust-mobile/xbuild.git` | pinned by git rev in script |

No Android Studio. No emulator image (opt-in via `--with-emulator`
flag — uses `sdkmanager "system-images;android-35;google_apis;x86_64"`
plus AVD creation). On-device testing via `x run --device adb:<id>`
over USB or local network, mirroring the Librem 5 cycle.

We deliberately do *not* write `~/.cargo/config.toml` cross-linker
entries: `xbuild` invokes the NDK linker itself via `ANDROID_NDK_ROOT`
detection. Adding global Cargo target stanzas would only matter for
direct `cargo build --target aarch64-linux-android` invocations
outside `x run`, which is not part of the documented Slint-on-Android
flow. Skip until proven necessary.

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
7. **Bells.** Audio playback via Android `MediaPlayer` JNI (or `oboe-rs`). Decision deferred to this milestone. Drives UI phase 5.
8. **Haptics.** Android `Vibrator` / `VibratorManager` JNI. Reuse `meditate-core` envelope-quantising logic; map quantised levels to Android amplitude scale (0-255). Pairs with UI phase 5.
9. ✅ **Foreground service + notification** — Kotlin `MediaSessionService` started via JNI bridge with classloader walk (`76dbe6f`), MediaStyle pin so the notification lives in the shade's Media-controls section (`ae6104f`). Phase-5 audio playback plugs into the same notification when it lands.
10. **Keychain.** Android `KeyStore` JNI for the Nextcloud app-password. `oo7` is host-only. Drives UI phase 7's password row.
11. **Sync.** `meditate-core::sync` already abstracts the HTTP layer (`ureq`). Plug in the JNI keychain and run. Pairs with UI phase 7.
12. **Polish: edge-to-edge, predictive back, IME insets, theming.** Predictive-back partly done (root `FocusScope` routes `Key.Back` through a Rust handler that closes the labels chooser → discards Done → swallows back during Running, commit `9e95a5d`). Edge-to-edge / IME insets / theming still pending.
13. **F-Droid metadata + reproducible build.** Manifest, fastlane structure, `metadata/io.github.janekbt.Meditate.yml` for fdroiddata. Lands after every UI phase is at least partly usable.

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

GTK reference: `meditate-gtk/src/stats/imp.rs`. Most of this is
rendering — every metric already has a `*_from_db` reader in
`meditate-core`. The contribution heatmap is the highest-effort
custom-painted bit.

To land:
- Numbers tile grid (total minutes, current streak, median, longest
  session).
- Daily-totals chart (bar chart per day for the active period).
- Contribution heatmap (the 91-cell calendar grid, accessibility via
  the existing `ContribCell::speech_role` backlog item).
- Label aggregate row.
- Hour-of-day buckets.
- Period selector (week / 4 weeks / 3 months / 1 year — mirrors GTK).

Verification: numbers match the GTK shell on the same DB; heatmap
matches visually for the same period range.

### Phase 5 — Bells + vibration patterns

Two-layer phase. Systems milestones 7 + 8 (audio + haptics JNI)
land in parallel with the UI work. GTK reference: bell-sound chooser
(`meditate-gtk/src/sounds.rs`), vibration-pattern chooser
(`meditate-gtk/src/vibrations.rs`). The `PreviewToggle` machinery is
already typed in core (`meditate_core::sound::PreviewToggle`).

To land:
- Bell-sound chooser screen (per-row Play/Stop preview hooked via
  `PreviewToggle`).
- Vibration-pattern chooser screen (same shape).
- Starting / Interval / End bell rows in Setup view.
- Box-Breath per-phase cue config (re-uses the Setup-view rows in
  the Box-Breath section).
- Starting/Interval/End bell sounds + vibration patterns play on
  the phone during a real session.

Verification: starting a Timer countdown with all three bells +
vibration enabled fires all three sounds + vibration at the right
phase boundaries.

### Phase 6 — Editors + Presets

GTK reference: vibration pattern editor
(`meditate-gtk/src/vibration_editor.rs`), preset chooser /
manage page (`meditate-gtk/src/presets.rs`), label chooser
(`meditate-gtk/src/labels.rs`).

To land:
- Vibration pattern editor (curve editor with point dragging — the
  math is already in `meditate_core::vibration`; just need the touch
  handlers).
- Preset chooser (chip list above the duration picker).
- Save / Override / Manage preset flow.
- Label chooser screen + per-row rename / delete with Undo.

Verification: a custom pattern authored on the phone syncs to the
laptop and renders identically in the GTK shell's editor.

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
