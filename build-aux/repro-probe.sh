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
    # Build B the way F-Droid does: with the signing config regex-stripped
    # out. This is what caught the 26.8.0 breakage only after their CI ran.
    if [ "$d" = "$B" ]; then
        say "strip signing config in $d (as fdroidserver does)"
        python3 "$SRC/build-aux/fdroid-strip-signing.py" "$d"
    fi
    say "build release in $d"
    # rust-build.sh applies the --remap-path-prefix flags itself now, so
    # nothing extra is exported here. Keep it that way: an exported
    # RUSTFLAGS accumulated across this loop would hand the second build
    # the first build's remap, which is the opposite of what a
    # determinism probe wants.
    ( cd "$d/meditate-android/android" && ./gradlew --no-daemon -q assembleRelease )
done

# Stripping the signing config leaves AGP with nothing to sign with, so
# that build emits app-release-unsigned.apk instead — same as on F-Droid's
# buildserver. Take whichever name exists.
find_apk() {
    local d="$1/meditate-android/android/app/build/outputs/apk/release"
    local f
    for f in "$d/app-release.apk" "$d/app-release-unsigned.apk"; do
        [ -f "$f" ] && { echo "$f"; return 0; }
    done
    echo "no APK in $d (build failed?)" >&2
    return 1
}
APK_A="$(find_apk "$A")"
APK_B="$(find_apk "$B")"

say "compare $(basename "$APK_A") vs $(basename "$APK_B")"
python3 "$SRC/build-aux/repro-compare.py" "$APK_A" "$APK_B"
