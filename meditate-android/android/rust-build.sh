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
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TB/aarch64-linux-android26-clang"
export CC_aarch64_linux_android="$TB/aarch64-linux-android26-clang"
export CXX_aarch64_linux_android="$TB/aarch64-linux-android26-clang++"
export AR_aarch64_linux_android="$TB/llvm-ar"

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"   # repo root
PROFILE="${1:-debug}"
cd "$ROOT"

if [ "$PROFILE" = "release" ]; then
    cargo build -p meditate-android --target aarch64-linux-android --release
    SO="target/aarch64-linux-android/release/libmeditate_android.so"
else
    cargo build -p meditate-android --target aarch64-linux-android
    SO="target/aarch64-linux-android/debug/libmeditate_android.so"
fi

DEST="$ROOT/meditate-android/android/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$DEST"
cp -f "$SO" "$DEST/libmeditate_android.so"
echo "rust-build: $PROFILE -> $DEST/libmeditate_android.so"
