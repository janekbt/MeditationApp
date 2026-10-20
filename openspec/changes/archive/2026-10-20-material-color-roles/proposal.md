# Material 3 colour roles on Android

## Why
An audit of the Android UI against Material 3 found places that use
colours Material has no role for (darkened containers, a darkened
button), text on the wrong role, a fixed scrim and shadow, an
outlined pill that doesn't look like an outlined button, and a
home-screen widget that ignores the system colours. The five fixed
accents only approximate Material's tonal palettes in HSL and keep
the blue-tinted surfaces under every accent. The maintainer wants all
of it to follow Material 3.

## What Changes
- Child rows inside expanded groups use the `surface-container-low` role; hover/press is an `on-surface` state layer.
- Text on a selected segment or selected preset row uses `on-secondary-container`.
- The filled wrap button's pressed state is a 10 % `on-primary` state layer; the outlined one a 10 % `primary` layer.
- The Play/Stop pill looks like an outlined button: `primary` text, 8 % `primary` on hover, 10 % pressed.
- Dialog and sheet scrims are the `scrim` role at 32 %; the snackbar shadow uses the `shadow` role.
- Green, Teal, Purple, Rose and Amber are Material's tonal-spot scheme from a seed, every role, surfaces included, via the `material-colors` crate. Blue stays the shipped baseline; System stays the phone's scheme.
- The home-screen widget takes the system colours on Android 12+, light and dark; below 12 it keeps today's slate.
- Left out: the black running screen and the chart/heatmap transparency (deliberate, Material has no role); the label colours (already derived from the accent's primary); other rows' hover fill (`surface-container-high`), not in the audit; the Material library's own 50 % modal backdrop (`material-1.0/` is build environment).

## Decisions
- Scope: everything from the audit, including the widget and HCT accents (maintainer).
- HCT through the crate `material-colors`, not an own port (maintainer). Licence MIT/Apache-2.0, fine with GPL-3.0. Version 0.4.2: 0.5.0 needs Rust 1.97, the toolchain is 1.95. Its required dependencies `ahash` and `indexmap` are already in Cargo.lock.
- Widget follows the system colours on Android 12+ (maintainer), XML only.
- Child rows: `surface-container-low` (maintainer).
- (my call) Seeds are today's swatch colours, so each accent keeps its hue family; tonal spot only takes the seed's hue.
- (my call) Tonal spot at standard contrast, the same variant Android uses for wallpaper colours; the crate implements the 2023 spec, the one Android 14 ships.
- (my call) Blue stays the shipped baseline verbatim: it already is a Material 3 scheme and the default below Android 14.
- (my call) State layers are mixed in with Slint's `mix()`, so the row stays opaque.
- (my call) Scrim 32 % as Material 3 says, even though the library's date and time pickers keep their 50 %.
- (my call) Widget roles: background `system_neutral1_50`/`_900`, text `system_neutral1_900`/`_50`, muted text `system_neutral2_700`/`_200`, row `system_accent2_100`/`_800`, the colours Android's own widgets use.

## Capabilities
### New Capabilities
- `appearance`: which Material 3 role each kind of element uses, and the widget's colours.
### Modified Capabilities
- `preferences`: a fixed accent is a full tonal-spot scheme from its seed.

## Impact
- Core: none.
- GTK: none. It follows libadwaita, not Material.
- Android: `src/theme.rs` (seeds, scheme from the crate, swatch), `ui/main.slint` (roles), `android/app/src/main/res/values-v31/colors.xml` and `values-night-v31/colors.xml` (new).
- Translations: none.
- Data: none. The stored accent values don't change.
- Android build environment: `meditate-android/Cargo.toml` gains `material-colors = "0.4.2"`, `Cargo.lock` gains that one crate. Repro probe on the final tree before the push.
- Docs: README feature line if it names the accents; `meditate-android/README` or BUILDING only if they list dependencies.
