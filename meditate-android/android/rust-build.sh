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

. "$HOME/.config/meditate-android/env.sh" 2>/dev/null || true
: "${ANDROID_NDK_ROOT:?ANDROID_NDK_ROOT not set (source env.sh)}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_SDK_ROOT="$ANDROID_HOME"

TB="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin"

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"   # repo root
PROFILE="${1:-debug}"
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
        SO="target/$TARGET/release/libmeditate_android.so"
    else
        cargo build -p meditate-android --target "$TARGET"
        SO="target/$TARGET/debug/libmeditate_android.so"
    fi

    DEST="$ROOT/meditate-android/android/app/src/main/jniLibs/$ABI"
    mkdir -p "$DEST"
    cp -f "$SO" "$DEST/libmeditate_android.so"
    echo "rust-build: $PROFILE $ABI -> $DEST/libmeditate_android.so"
done
