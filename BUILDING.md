# Building, testing, deploying

The one reference for building Meditate from source. [`README.md`](README.md)
only points here; the release runbook is [`RELEASE.md`](RELEASE.md).
Commands are written for the maintainer's machine (Debian 13 trixie,
x86_64) and run from the repository root unless a `cd` says otherwise.

| I want to … | Command | Section |
|---|---|---|
| build + run the Linux app | `./build.sh gtk --debug` → `./meditate-gtk/builddir/src/meditate` | [Linux app](#linux-app) |
| build the Linux Flatpak | `flatpak-builder --user --install --force-clean --install-deps-from=flathub flatpak_app build-aux/io.github.janekbt.Meditate.json` | [Linux app](#linux-app) |
| run all tests | `cargo test --workspace` | [Tests and lints](#tests-and-lints) |
| build + install the Android debug APK | `./build.sh android --debug` then `adb install -r meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` | [Android](#android) |
| regenerate the GTK translation template | `build-aux/update-translations.sh` | [Translations](#translations) |
| cross-build for the Librem 5 | `build-aux/dev-xbuild.sh` | [Librem 5](#librem-5) |
| cut a release | follow [`RELEASE.md`](RELEASE.md) | — |

> **F-Droid reproducibility.** F-Droid rebuilds every release and ships the
> maintainer's signed APK only if its rebuild matches byte for byte. Never
> change these without a reproducibility check (`build-aux/repro-probe.sh`):
> `meditate-android/android/rust-build.sh`, the Gradle files under
> `meditate-android/android/`, `meditate-android/build.rs`, `third_party/`,
> `rust-toolchain.toml`, `Cargo.lock`, and the pins they reference (JDK 17,
> build-tools 31.0.0, platform 34, NDK r27c). Workarounds for local builds
> belong on the command line, never in those files.

## Prerequisites

### Rust

[rustup](https://rustup.rs). `rust-toolchain.toml` pins the toolchain
(1.95.0); rustup installs it on the first `cargo` run inside the repo.
Targets must be added **from inside the repo** so they land on that pinned
toolchain rather than rustup's default.

### Linux app

GTK ≥ 4.18 and libadwaita ≥ 1.7 (Debian 13, Fedora 42, Arch are fine;
Ubuntu 24.04 LTS, Fedora 40 and Debian 12 are too old — use the
[Flatpak build](#flatpak-build) there), blueprint-compiler ≥ 0.16, meson,
ninja, gettext, GStreamer.

```sh
# Debian / Ubuntu
sudo apt install build-essential meson ninja-build pkg-config \
    libgtk-4-dev libadwaita-1-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev blueprint-compiler \
    desktop-file-utils gettext
# Fedora
sudo dnf install gcc meson ninja-build pkgconf-pkg-config gtk4-devel \
    libadwaita-devel gstreamer1-devel gstreamer1-plugins-base-devel \
    blueprint-compiler desktop-file-utils gettext
# Arch
sudo pacman -S --needed base-devel meson ninja pkgconf gtk4 libadwaita \
    gstreamer gst-plugins-base blueprint-compiler desktop-file-utils gettext
```

`./build.sh gtk` installs exactly these when something is missing (it asks
for your password once) and skips the step when everything is present.

### Android

**JDK 17 — exactly 17.** `rust-build.sh` pins the d8 dexer from build-tools
31.0.0 so the APK matches F-Droid's rebuild, and that d8 crashes on classes
compiled by a newer javac (`Dex conversion failed`). Debian 13 no longer
ships 17; add Debian 12's archive for it, pinned low so nothing else is ever
taken from it — the same thing F-Droid's build recipe does:

```sh
echo "deb https://deb.debian.org/debian bookworm main" | sudo tee /etc/apt/sources.list.d/bookworm.list
printf 'Package: *\nPin: release n=bookworm\nPin-Priority: 100\n' | sudo tee /etc/apt/preferences.d/bookworm-low
sudo apt update && sudo apt install openjdk-17-jdk-headless
```

(Fedora: `java-17-openjdk-devel`; Arch: `jdk17-openjdk`.) Your system
default Java can stay whatever it is.

Then the toolchain, one time:

```sh
build-aux/setup-android.sh
```

Idempotent. It downloads the pinned Android command-line tools, SDK
platforms 34 and 35, build-tools 31.0.0 and 35.0.0, NDK r27c
(27.2.12479018), Gradle 8.5 and kotlinc into `~/Android`, adds the Rust
Android targets to the pinned toolchain, and writes
`~/.config/meditate-android/env.sh` (with `JAVA_HOME` = JDK 17) plus a line
in `~/.bashrc` that sources it. Re-run it after changing any pin; an
`env.sh` written by an older version of the script points at the wrong JDK.

Release signing (maintainer only): `~/.config/meditate-android/signing.properties`
plus the keystore it names. Without it, `assembleRelease` signs with the
standard debug keystore. The keystore is the app's identity on F-Droid —
losing it means no update can ever reach installed users.

## Linux app

### Native build

```sh
./build.sh gtk             # optimized
./build.sh gtk --debug     # faster to build
./meditate-gtk/builddir/src/meditate
```

Manual equivalent:

```sh
cd meditate-gtk
meson setup builddir --buildtype=debug   # once
ninja -C builddir
./builddir/src/meditate
sudo ninja -C builddir install           # optional, system-wide (use --prefix=/usr at setup)
```

If meson refuses an existing `builddir` ("generated with Meson version …,
which is incompatible"), recreate it: `meson setup --wipe builddir`.

### Flatpak build

Needs `flatpak`, `flatpak-builder` and the Flathub remote
(`flatpak remote-add --if-not-exists --user flathub https://flathub.org/repo/flathub.flatpakrepo`).

```sh
flatpak-builder --user --install --force-clean --install-deps-from=flathub \
    flatpak_app build-aux/io.github.janekbt.Meditate.json
flatpak run io.github.janekbt.Meditate
```

`--install-deps-from=flathub` pulls the GNOME 50 runtime/SDK and the
`rust-stable` extension on first run. `--install` replaces any installed
Meditate Flatpak. The build runs cargo offline against
`build-aux/cargo-sources.json`, which must list every crate in `Cargo.lock`
— see [RELEASE.md](RELEASE.md) step 1 for regenerating it.

## Tests and lints

```sh
cargo test --workspace                                   # everything (~1,240 tests)
cargo test -p meditate-core -p meditate-android --lib    # what CI runs (no GTK libs needed)
cargo clippy -p meditate-core -p meditate-android --lib -- -D warnings   # what CI runs
cargo clippy -p meditate -p meditate-core --all-targets  # GTK shell too
```

- `cargo test --workspace` needs the GTK/libadwaita development packages and
  blueprint-compiler, but not meson.
- The GTK shell's cargo package is named **`meditate`**, not `meditate-gtk`.
- Most of `meditate-android` is compiled only for Android, so host clippy
  doesn't see it. To lint it, run clippy for the Android target with the NDK
  toolchain from `env.sh`:

  ```sh
  . ~/.config/meditate-android/env.sh
  TB="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin"
  CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TB/aarch64-linux-android26-clang" \
  CC_aarch64_linux_android="$TB/aarch64-linux-android26-clang" \
  CXX_aarch64_linux_android="$TB/aarch64-linux-android26-clang++" \
  AR_aarch64_linux_android="$TB/llvm-ar" \
      cargo clippy -p meditate-android --target aarch64-linux-android
  ```

  CI does not run this; the warnings it reports predate this document.

## Android

The APK is built by the hand-maintained Gradle project in
`meditate-android/android/` (AGP 7.3, Gradle 8.5 via the committed wrapper,
compileSdk/targetSdk 34, minSdk 26, arm64-v8a). Its `cargoNdkBuild` task runs
`rust-build.sh`, which compiles the Rust library with plain
`cargo build --target aarch64-linux-android` (not cargo-ndk, whose 4.x
runner breaks slint's build script) and drops it into `jniLibs/`.

```sh
./build.sh android --debug    # debug APK → meditate-android/android/app/build/outputs/apk/debug/app-debug.apk
./build.sh android            # release APK → …/apk/release/app-release.apk
```

Manual equivalent:

```sh
. ~/.config/meditate-android/env.sh
cd meditate-android/android
./gradlew :app:assembleDebug          # or :app:assembleRelease
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

`adb install -r` keeps the app's data. A debug or CI build cannot update an
F-Droid install (different signing key); uninstalling first deletes the
data. For an emulator APK: `ABIS="arm64-v8a x86_64" ./gradlew :app:assembleDebug`.

Troubleshooting:

- **`Dex conversion failed` … `JDK version 21 is known to cause an error`** —
  the build ran on a JDK other than 17. Check `echo $JAVA_HOME`; re-run
  `build-aux/setup-android.sh` to rewrite `env.sh`, or prefix the command
  with `JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64`.
- **APK timestamp didn't change after a rebuild** — Gradle compares content:
  if the stripped native library is byte-identical, packaging is skipped.
  That's expected, not a stale build.
- **Reproducibility** — `build-aux/repro-probe.sh` builds the release APK
  twice from two differently long paths (the second with the signing config
  stripped, as F-Droid does) and compares them. Run it after touching
  anything in the protected list above.

## Translations

- **GTK:** `meditate-gtk/po/` — template `meditate.pot`, one `<lang>.po` per
  language in `LINGUAS`. Every source file with a `gettext(` / `ngettext(`
  call or a `_("` / `translatable="yes"` marker must be listed in
  `po/POTFILES.in`. Regenerate the template and merge it into every
  `<lang>.po` with:

  ```sh
  build-aux/update-translations.sh
  ```

  It runs the `meditate-pot` and `meditate-update-po` meson targets inside
  the GNOME SDK the Flatpak manifest pins (installed by the
  [Flatpak build](#flatpak-build)), so the result doesn't depend on the
  distro's gettext. Don't run those targets from a host build directory:
  xgettext before 0.24 reads `.rs` files as C and gets some strings wrong.
  Then translate the new and fuzzy entries.

  The SDK's xgettext skips calls it can't see as plain `gettext(…)`: write
  `gettext("…")` / `ngettext(…)` after `use crate::i18n::{gettext, ngettext};`,
  never `crate::i18n::gettext(…)`, and use `clone!` (after `use glib::clone;`)
  rather than `glib::clone!` around code that has translatable strings.
  `cargo test -p meditate i18n::` fails on either mistake, on a file missing
  from `POTFILES.in`, and on a stale template.
- **Android:** `meditate-android/lang/<lang>/LC_MESSAGES/meditate-android.po`,
  bundled into the binary at build time; strings come from `@tr()` in the
  `.slint` files and the `Tr` catalogue in `ui/main.slint`.
- Validate every file you touched: `msgfmt --check -o /dev/null <file>`.

## Librem 5

### Cross-compile

`build-aux/dev-xbuild.sh` cross-compiles a Librem 5 binary in seconds instead
of the 20–35 minute `flatpak-builder --arch=aarch64` QEMU build. Output:
`target/aarch64-unknown-linux-gnu/release/meditate`.

One-time prerequisites (also in the script header):

```sh
rustup target add aarch64-unknown-linux-gnu      # from inside the repo
sudo apt install gcc-aarch64-linux-gnu
flatpak install --user --arch=aarch64 flathub org.gnome.Sdk//50
mkdir -p ~/sysroots/gnome50-aarch64
ln -sfn ~/.local/share/flatpak/runtime/org.gnome.Sdk/aarch64/50/active/files \
        ~/sysroots/gnome50-aarch64/usr
```

The maintainer's existing `~/sysroots/gnome50-aarch64/usr` is a hand-built
directory (per-file links plus patched linker scripts), not this symlink. If
linking fails after an SDK update, recreate it with the commands above.

### Deploy over SSH

Always wrap SSH/scp in a timeout — a suspended phone otherwise hangs each
call for about two minutes (exit code 124 means unreachable: wake the phone).

1. Stop the app and remove the old binary. scp can't overwrite a running
   executable (`dest open …: Failure` is `ETXTBSY`); a removed file stays
   valid for the running process.

   ```sh
   timeout 8 ssh -o ConnectTimeout=5 purism@<phone-ip> \
     'pkill -x meditate; sleep 0.5; rm -f /home/purism/.local/share/flatpak/app/io.github.janekbt.Meditate/current/active/files/bin/meditate'
   ```

   Use `pkill -x meditate`, not `pkill -f bin/meditate` — `-f` matches the
   SSH shell's own command line and kills the session (exit 255).

2. Copy the new binary:

   ```sh
   timeout 30 scp -o ConnectTimeout=5 target/aarch64-unknown-linux-gnu/release/meditate \
     purism@<phone-ip>:/home/purism/.local/share/flatpak/app/io.github.janekbt.Meditate/current/active/files/bin/meditate
   ```

3. Only if a development build left the test phone's database in a state
   it can't read, reset it (this also deletes the sync settings; only the
   keyring password survives):

   ```sh
   timeout 8 ssh -o ConnectTimeout=5 purism@<phone-ip> \
     'rm -f ~/.var/app/io.github.janekbt.Meditate/data/meditate/meditate.db{,-shm,-wal}'
   ```

   Schema changes ship with a migration ([`DECISIONS.md`](DECISIONS.md)
   rule 3), so this is not part of the normal cycle.

## CI

`.github/workflows/flatpak.yml` runs on pushes and pull requests to `main`,
and manually on any branch:

```sh
gh workflow run flatpak.yml --ref beta               # everything (~35 min)
gh workflow run flatpak.yml --ref beta -f scope=light  # skip the two Flatpak builds
```

Jobs: metainfo + desktop-file validation, `cargo test` + `clippy -D warnings`
for core and android, an Android release APK (JDK 21 and only build-tools 35
on the runner, so its APK is not the reproducible one — and signed with the
debug keystore), and the x86_64 and aarch64 Flatpak bundles. The bundles of
a tagged commit's run are what a release ships to Linux users.
