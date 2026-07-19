# Building, cross-compiling, deploying

The root [`README.md`](README.md#building-from-source) covers the
two canonical paths (Flatpak local build, native Meson build). This
file is the working notes for two paths that aren't covered there:

1. **Cross-compiling for the Librem 5 (aarch64)** for fast iteration
   without a 20-minute QEMU round-trip.
2. **Deploying to the phone over SSH** — the kill/rm + DB-wipe +
   timeout-wrap dance that took several iterations to settle.

If you're only building on the host where the app will run, you
don't need this file.

## Cross-compile to aarch64 for the Librem 5

`build-aux/dev-xbuild.sh` cross-compiles a Librem 5–compatible
binary in ~15 seconds on an x86_64 host. Output:
`target/aarch64-unknown-linux-gnu/release/meditate`, ready to scp
straight over a Flatpak-installed binary on the phone.

The script self-documents the one-time prerequisites in its
header. The short version: `aarch64-unknown-linux-gnu` Rust target
+ a cross-compiling linker + the GNOME runtime's aarch64 libs.

Use this instead of `flatpak-builder --arch=aarch64` (which uses
QEMU and takes 20–35 min) whenever you're iterating on perf-
sensitive code that needs real-device runs (haptic timing,
suspend behaviour, anything where the laptop can't faithfully
verify the change).

## Deploy to the Librem 5 over SSH

Three habits worth getting in muscle memory. The first two are
load-bearing; the third is just a quality-of-life rule that
prevents 2-minute hangs.

### 1. Kill the running app + rm the old binary before scp

scp overwrite-in-place onto a running executable fails with the
cryptic `scp: dest open ...: Failure` — that's `ETXTBSY` (text
file busy). Removing the file works while the app is running
because the running process keeps the inode; the next launch
picks up the new file.

```bash
timeout 8 ssh -o ConnectTimeout=5 purism@<phone-ip> \
  'pkill -x meditate; sleep 0.5; \
   rm -f /home/purism/.local/share/flatpak/app/io.github.janekbt.Meditate/current/active/files/bin/meditate'
```

**Use `pkill -x meditate`, NOT `pkill -f bin/meditate`.** The `-f`
form matches the SSH'd shell's own argv (it contains the literal
pattern) and kills the session before the rm runs, exit 255.

Then the scp:

```bash
timeout 30 scp -o ConnectTimeout=5 \
  target/aarch64-unknown-linux-gnu/release/meditate \
  purism@<phone-ip>:/home/purism/.local/share/flatpak/app/io.github.janekbt.Meditate/current/active/files/bin/meditate
```

### 2. Wipe the local DB after any schema or wire-format change

This codebase follows the "no backwards-compat" rule (see
[`DECISIONS.md`](DECISIONS.md) rule 3). Several recent passes have
shipped schema changes (column rename `interval_bells.sound` →
`sound_uuid`, typed-UUID newtypes) where a reused DB would carry
stale rows that no longer round-trip through the new lookup
paths.

```bash
timeout 8 ssh -o ConnectTimeout=5 purism@<phone-ip> \
  'rm -f ~/.var/app/io.github.janekbt.Meditate/data/meditate/meditate.db{,-shm,-wal}'
```

On the laptop the path is `~/.local/share/meditate/meditate.db{,-shm,-wal}`
— same pattern, no flatpak prefix.

**Heads-up:** wiping the DB also takes the Nextcloud sync URL,
username, sync path, and interval with it (those live in the
`settings` table). Only the keyring password survives — from the
user's perspective, the sync config is gone and they have to
re-enter it. Warn before wiping if you're not the user.

### 3. Always wrap SSH / scp with a wall-clock timeout

Phones suspend, drop off WiFi, or are otherwise unreachable.
Without a timeout the bash command waits ~2 minutes per call,
which is a terrible experience when iterating.

```bash
timeout 8  ssh -o ConnectTimeout=5 purism@<phone-ip> '...'   # for one-line commands
timeout 30 scp -o ConnectTimeout=5 <file> purism@<phone-ip>:<path>  # for transfers
```

If `timeout` exits 124, the phone is unreachable — wake it
rather than retrying mechanically.

## Android

The Android app (`meditate-android/`) builds through a hand-
maintained Gradle project — **not** xbuild/cargo-apk (xbuild was
removed 2026-06; cargo-ndk 4.x breaks Slint's build.rs SDK lookup,
so a plain `cargo build --target` drives the NDK toolchain
directly).

### One-time setup

```sh
build-aux/setup-android.sh
```

Idempotent (Debian/Ubuntu): installs JDK 21, the Android SDK
(platform 34, build-tools 35), NDK r27, Gradle 8.5 and kotlinc,
and writes the pinned paths to `~/.config/meditate-android/env.sh`
— the file every Android build command sources. No Android Studio
required. Then add the Rust target:

```sh
rustup target add aarch64-linux-android
```

### Build + install (debug)

```sh
cd meditate-android/android
. ~/.config/meditate-android/env.sh
./gradlew :app:assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

The `cargoNdkBuild` Gradle task runs `rust-build.sh`, which
compiles the Rust cdylib and drops it into `jniLibs/` before AGP
packages the APK. Default ABI is `arm64-v8a`; for an emulator
build: `rustup target add x86_64-linux-android` once, then
`ABIS="arm64-v8a x86_64" ./gradlew :app:assembleDebug`.

`assembleRelease` produces an optimized build signed with the
standard Android debug keystore (fine for sideloading and
measurement; F-Droid re-signs with its own key).

### Tests

```sh
cargo test -p meditate-core -p meditate-android --lib
```

runs the full core + Android-shell suite on the host — no device
needed (the JNI/Slint layers are cfg-gated out).

### References

The canonical Slint docs are at
<https://material.slint.dev/getting-started/> (Material) and
docs.slint.dev (Android backend — search "Android" in the side
nav).
