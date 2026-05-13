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

## Cross-compile to Android (xbuild)

The Android port lives on the `android` branch and uses `xbuild`
(the `x` CLI from rust-mobile/xbuild). Two gotchas worth
documenting since they diverge from the canonical Slint Material
Rust template (which targets `cargo-apk`):

### Lib name must equal the package name (no `_lib` suffix)

The Slint Material template has:

```toml
[package]
name = "my_app"
[lib]
name = "my_app_lib"
```

`cargo-apk` tolerates this via `cargo apk run --lib`. **xbuild
does NOT** — its APK packaging step looks for
`lib<package_name_with_underscores>.so` and fails with
`failed to locate bin libmy_app.so`. Drop the `_lib` suffix:

```toml
[lib]
name = "my_app"
```

`src/main.rs` calls `my_app::main()` rather than
`my_app_lib::main()`.

### xbuild needs `llvm-readobj` on PATH

The APK packaging step shells out to `llvm-readobj --needed-libs
<lib>` to walk the cdylib's NEEDED entries. `llvm-readobj` ships
in the NDK's clang toolchain at:

```
$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/
```

NOT in the SDK side. Add this directory to PATH; without it
xbuild dies with `Failed to run llvm-readobj ... No such file or
directory`.

The canonical Slint docs are at
<https://material.slint.dev/getting-started/> (Material) and
docs.slint.dev (Android backend — search "Android" in the side
nav).
