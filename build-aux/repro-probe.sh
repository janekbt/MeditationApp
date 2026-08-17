#!/usr/bin/env bash
# Reproducibility probe: build the release APK twice, from two clones at
# DIFFERENT absolute paths (deliberately different lengths), and compare
# the payloads entry by entry.
#
# Why different paths: rustc bakes absolute source and $CARGO_HOME paths
# into panic/debug metadata, which is the most common reason a Rust app
# fails F-Droid's reproducible-build verification. Same-path rebuilds hide
# exactly that class of bug.
#
# Scope: this is a probe, not proof. It runs on one machine with one
# toolchain, so it cannot tell us whether F-Droid's buildserver produces
# the same bytes; it tells us whether the build is path-independent and
# deterministic here — the prerequisite for anything else.
#
# Signature material (META-INF) is excluded: F-Droid's own verification
# compares payloads, not signature blocks.
set -euo pipefail

WORK="${WORK:-$HOME/.cache/meditate-repro}"
SRC="$(cd "$(dirname "$0")/.." && pwd)"
REF="${REF:-HEAD}"
# Deliberately different path lengths.
A="$WORK/a"
B="$WORK/b-a-considerably-longer-path-segment"

# The laptop has a 15 GiB ceiling and heavy builds have locked it before.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

say() { printf '\n=== %s\n' "$*"; }

rm -rf "$WORK"
mkdir -p "$WORK"

for d in "$A" "$B"; do
    say "clone -> $d"
    git clone --quiet --shared --no-checkout "$SRC" "$d"
    git -C "$d" checkout --quiet "$REF"
done

for d in "$A" "$B"; do
    say "build release in $d"
    # REMAP=1 applies the candidate fix: rustc bakes the absolute path of
    # build-script-generated sources (slint's main.rs, glutin's
    # egl_bindings.rs) and of the cargo registry into the binary.
    # --remap-path-prefix rewrites both to fixed placeholders, so two
    # builds at different paths should emit identical bytes.
    if [ "${REMAP:-0}" = 1 ]; then
        export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$d=/build --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
    fi
    ( cd "$d/meditate-android/android" && ./gradlew --no-daemon -q assembleRelease )
done

APK_A="$A/meditate-android/android/app/build/outputs/apk/release/app-release.apk"
APK_B="$B/meditate-android/android/app/build/outputs/apk/release/app-release.apk"

say "compare"
python3 "$SRC/build-aux/repro-compare.py" "$APK_A" "$APK_B"
