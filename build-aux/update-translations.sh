#!/usr/bin/env bash
# Regenerate the GTK translation template (meditate-gtk/po/meditate.pot)
# and merge it into every <lang>.po.
#
# Runs inside the GNOME SDK the Flatpak manifest pins, so everyone extracts
# with the same gettext version whatever their distro ships. That matters:
# xgettext older than 0.24 reads .rs files as C (wrong msgids for strings
# continued with `\`, bogus c-format flags), and newer ones differ in what
# they recognise. The `i18n::tests` in meditate-gtk fail if a string would
# be missed or the template is stale.
#
# Usage: build-aux/update-translations.sh
#
# Needs flatpak with the GNOME SDK and its rust-stable extension, which the
# Flatpak build (BUILDING.md) installs on first run.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="$PWD"

SDK_BRANCH=$(sed -n 's/^ *"runtime-version": *"\([^"]*\)".*/\1/p' \
    build-aux/io.github.janekbt.Meditate.json)
SDK="runtime/org.gnome.Sdk/x86_64/$SDK_BRANCH"

# `flatpak run` refuses to guess when the SDK is installed both per-user and
# system-wide, so pick one explicitly.
if flatpak info --user "$SDK" >/dev/null 2>&1; then
    INSTALLATION=--user
elif flatpak info --system "$SDK" >/dev/null 2>&1; then
    INSTALLATION=--system
else
    echo "GNOME SDK $SDK_BRANCH not installed. Run the Flatpak build once" >&2
    echo "(BUILDING.md -> Flatpak build) or:" >&2
    echo "  flatpak install --user flathub org.gnome.Sdk//$SDK_BRANCH" >&2
    exit 1
fi

# A throwaway meson build directory, so po/meson.build stays the one place
# the xgettext arguments live.
BUILD=$(mktemp -d "$REPO/meditate-gtk/.i18n-build.XXXXXX")
trap 'rm -rf "$BUILD"' EXIT

flatpak run "$INSTALLATION" --filesystem="$REPO" --command=sh "$SDK" -c '
    set -e
    export PATH="/usr/lib/sdk/rust-stable/bin:$PATH"
    command -v cargo >/dev/null || {
        echo "rust-stable SDK extension missing; run the Flatpak build once" >&2
        exit 1
    }
    xgettext --version | head -1
    meson setup "$1" "$2/meditate-gtk" >/dev/null
    meson compile -C "$1" meditate-pot meditate-update-po
    for po in "$2"/meditate-gtk/po/*.po; do
        msgfmt --check -o /dev/null "$po"
    done
' sh "$BUILD" "$REPO"

echo "Updated meditate-gtk/po/. Translate the new and fuzzy entries, then run"
echo "  cargo test -p meditate i18n::"
