#!/usr/bin/env bash
# Builds the meditate-android cdylib for arm64-v8a and drops it
# into the Gradle app's jniLibs. Invoked by the `cargoNdkBuild`
# Gradle task (preBuild dependency).
#
# We do NOT use cargo-ndk: its 4.x runner sanitizes the build
# environment, which makes i-slint-backend-android-activity's
# build.rs fail to find the SDK platforms ("No Android platforms
# found"). Plain `cargo build --target` keeps ANDROID_HOME intact;
# we just set the per-ABI NDK linker/CC/AR that cargo-ndk would
# otherwise have provided. API level 26 = our minSdk (forward-
# compatible up to API 35 devices).
set -euo pipefail

# env.sh exports JAVA_HOME unconditionally, so preserve a caller-set one:
# release builds need JDK 17 for the pinned dexer below, and without this
# they would silently fall back to whatever env.sh pins.
_caller_java_home="${JAVA_HOME:-}"
. "$HOME/.config/meditate-android/env.sh" 2>/dev/null || true
if [ -n "$_caller_java_home" ]; then
    export JAVA_HOME="$_caller_java_home"
    export PATH="$JAVA_HOME/bin:$PATH"
fi
# F-Droid's buildserver (and some CIs) export ANDROID_NDK_HOME /
# ANDROID_NDK instead of ANDROID_NDK_ROOT — accept any of them.
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-${ANDROID_NDK:-}}}"
: "${ANDROID_NDK_ROOT:?ANDROID_NDK_ROOT not set (source env.sh)}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_SDK_ROOT="$ANDROID_HOME"

TB="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin"

# slint's android-activity backend compiles a small Java helper and dexes it
# into the .so at build time, picking the newest build-tools it finds. That
# made our dex differ from F-Droid's (d8 8.6.2 here against 3.0.41 there) and
# was the last thing keeping their rebuild from matching our published APK.
# Pin the dexer to the version their buildserver uses. Note d8 3.0.41 rejects
# JDK-21 classfiles, so release builds need JAVA_HOME on JDK 17 (RELEASE.md).
# The helper is compiled against android.jar too, and d8 3.0.41 cannot read a
# platform-35 jar at all. Pin it to 34, which is both what their buildserver
# uses and this app's compileSdk.
JAR_PIN="$ANDROID_HOME/platforms/android-34/android.jar"
[ -f "$JAR_PIN" ] && export ANDROID_JAR="${ANDROID_JAR:-$JAR_PIN}"
D8_PIN="$ANDROID_HOME/build-tools/31.0.0/lib/d8.jar"
if [ -f "$D8_PIN" ]; then
    export ANDROID_D8_JAR="${ANDROID_D8_JAR:-$D8_PIN}"
else
    # Don't fail a contributor's build over this; only reproducibility suffers.
    echo "rust-build: warning: $D8_PIN missing, dex output will not be" >&2
    echo "rust-build:          reproducible (sdkmanager 'build-tools;31.0.0')" >&2
fi

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"   # repo root
PROFILE="${1:-debug}"
# Honour CARGO_TARGET_DIR; cargo writes there instead of $ROOT/target, and
# hardcoding the latter both fails the copy below and escapes the path
# remapping set up next.
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

# Reproducible builds: rustc bakes the absolute path of build-script
# output (slint's generated main.rs, glutin's egl_bindings.rs) and of
# every crates.io source file into the binary, so the same source built
# in two directories produces two different .so files — and because a
# longer path shifts .rodata, thousands of relocations shift with it.
# Rewriting both prefixes to fixed placeholders makes the output
# path-independent, which is what F-Droid's build verification needs;
# it also keeps the developer's home directory out of shipped APKs.
# Cargo deliberately excludes --remap-path-prefix from its metadata
# hash, so this does not perturb OUT_DIR names.
# Verified byte-identical across two differing paths — see
# build-aux/repro-probe.sh.
#
# The target dir is remapped to the same placeholder it gets as $ROOT/target,
# so a build with CARGO_TARGET_DIR pointing elsewhere still emits identical
# bytes rather than leaking that path into the generated sources.
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$TARGET_DIR=/build/target --remap-path-prefix=$ROOT=/build --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
# ABIS: space-separated list, default arm64 only (device builds).
# `ABIS="arm64-v8a x86_64" ./rust-build.sh` adds the emulator ABI —
# run `rustup target add x86_64-linux-android` once first.
ABIS="${ABIS:-arm64-v8a}"
cd "$ROOT"

for ABI in $ABIS; do
    case "$ABI" in
        arm64-v8a)   TARGET=aarch64-linux-android; TC=aarch64-linux-android26 ;;
        x86_64)      TARGET=x86_64-linux-android;  TC=x86_64-linux-android26 ;;
        armeabi-v7a) TARGET=armv7-linux-androideabi; TC=armv7a-linux-androideabi26 ;;
        *) echo "rust-build: unknown ABI $ABI" >&2; exit 1 ;;
    esac
    TRIPLE_ENV="$(echo "$TARGET" | tr 'a-z-' 'A-Z_')"
    export "CARGO_TARGET_${TRIPLE_ENV}_LINKER=$TB/${TC}-clang"
    export "CC_$(echo "$TARGET" | tr '-' '_')=$TB/${TC}-clang"
    export "CXX_$(echo "$TARGET" | tr '-' '_')=$TB/${TC}-clang++"
    export "AR_$(echo "$TARGET" | tr '-' '_')=$TB/llvm-ar"

    if [ "$PROFILE" = "release" ]; then
        cargo build -p meditate-android --target "$TARGET" --release
        SO="$TARGET_DIR/$TARGET/release/libmeditate_android.so"
    else
        cargo build -p meditate-android --target "$TARGET"
        SO="$TARGET_DIR/$TARGET/debug/libmeditate_android.so"
    fi

    DEST="$ROOT/meditate-android/android/app/src/main/jniLibs/$ABI"
    mkdir -p "$DEST"
    cp -f "$SO" "$DEST/libmeditate_android.so"
    echo "rust-build: $PROFILE $ABI -> $DEST/libmeditate_android.so"
done
