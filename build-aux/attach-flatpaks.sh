#!/usr/bin/env bash
# Attach the Flatpak bundles to a published GitHub release.
#
# Flathub is not an option for this app, so the GitHub release is the only way
# Linux users get a build. The bundles are already produced by the Flatpak CI
# run for the tagged commit, so this downloads those artifacts rather than
# rebuilding them locally (a local flatpak-builder run takes the better part of
# an hour and would produce a binary nobody verified).
#
# Usage: build-aux/attach-flatpaks.sh <version>      e.g. 26.9.0
#
# Requires the CI run for the tag's commit to have finished successfully.
set -euo pipefail

VERSION="${1:?usage: attach-flatpaks.sh <version>  (e.g. 26.9.0)}"
TAG="v$VERSION"
cd "$(dirname "$0")/.."

SHA="$(git rev-parse "$TAG^{commit}")"
echo "tag $TAG -> $SHA"

# The run must be for this exact commit: attaching bundles built from other
# source than the tag would ship users something the tag doesn't describe.
RUN=$(gh run list --workflow flatpak.yml --limit 30 \
        --json databaseId,headSha,conclusion \
        --jq "[.[] | select(.headSha==\"$SHA\" and .conclusion==\"success\")][0].databaseId")
if [ -z "$RUN" ] || [ "$RUN" = "null" ]; then
    echo "no successful Flatpak CI run for $SHA" >&2
    echo "wait for CI on the tagged commit, then re-run" >&2
    exit 1
fi
echo "using run $RUN"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

for arch in x86_64 aarch64; do
    name="meditate-$arch.flatpak"
    gh run download "$RUN" --name "$name" --dir "$WORK/$arch"
    file="$WORK/$arch/$name"
    [ -s "$file" ] || { echo "$name missing or empty in run $RUN" >&2; exit 1; }
    # Version the filename: release assets from different tags otherwise
    # collide in a browser's download folder as identical names.
    dest="$WORK/meditate-$VERSION-$arch.flatpak"
    mv "$file" "$dest"
    gh release upload "$TAG" "$dest" --clobber
    echo "attached $(basename "$dest") ($(du -h "$dest" | cut -f1))"
done

echo
echo "release assets now:"
gh release view "$TAG" --json assets --jq '.assets[] | "  \(.name)  \((.size/1048576)|floor)MB"'
