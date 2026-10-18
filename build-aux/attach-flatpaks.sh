#!/usr/bin/env bash
# Attach the Flatpak bundles and the FlatPark app trees to a published
# GitHub release.
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

# Download and check everything before the first upload, so a missing
# artifact never leaves the release half attached.
# FlatPark installs from the app tree tarball, not the bundle.
for arch in x86_64 aarch64; do
    for name in "meditate-$arch.flatpak" "meditate-$arch.tar.gz"; do
        gh run download "$RUN" --name "$name" --dir "$WORK/$arch" ||
            { echo "run $RUN has no $name (a scope=light run? re-run the full one)" >&2; exit 1; }
        [ -s "$WORK/$arch/$name" ] || { echo "$name missing or empty in run $RUN" >&2; exit 1; }
    done
done

# FlatPark pins a tarball by sha256 and size once it has seen it, so a
# replaced tarball would break every FlatPark install until the next release:
# an attached one with other content stops the script, the same one is kept.
declare -A upload_tarball
for arch in x86_64 aarch64; do
    # Version the filenames: release assets from different tags otherwise
    # collide in a browser's download folder as identical names.
    mv "$WORK/$arch/meditate-$arch.flatpak" "$WORK/meditate-$VERSION-$arch.flatpak"
    tarball="meditate-$VERSION-linux-$arch.tar.gz"
    mv "$WORK/$arch/meditate-$arch.tar.gz" "$WORK/$tarball"
    have="$(gh release view "$TAG" --json assets --jq ".assets[] | select(.name==\"$tarball\") | \"\\(.state) \\(.digest)\"")"
    want="uploaded sha256:$(sha256sum "$WORK/$tarball" | cut -d' ' -f1)"
    if [ -z "$have" ]; then
        upload_tarball[$arch]=1
    elif [ "${have%% *}" != uploaded ]; then
        # An upload that broke off leaves an asset nobody can download.
        echo "$tarball is half uploaded; delete it: gh release delete-asset $TAG $tarball" >&2
        exit 1
    elif [ "$have" != "$want" ]; then
        echo "$tarball is already attached with other content; FlatPark may have pinned it, not replacing" >&2
        exit 1
    fi
done

for arch in x86_64 aarch64; do
    gh release upload "$TAG" "$WORK/meditate-$VERSION-$arch.flatpak" --clobber
    echo "attached meditate-$VERSION-$arch.flatpak"
    tarball="meditate-$VERSION-linux-$arch.tar.gz"
    if [ -n "${upload_tarball[$arch]:-}" ]; then
        gh release upload "$TAG" "$WORK/$tarball"
        echo "attached $tarball"
    else
        echo "$tarball already attached, same file"
    fi
done

echo
echo "release assets now:"
gh release view "$TAG" --json assets --jq '.assets[] | "  \(.name)  \((.size/1048576)|floor)MB"'
