#!/usr/bin/env bash
# bump-version.sh — mechanical half of a release (see RELEASE.md
# for the full runbook this belongs to).
#
# Stamps ONE date-based version everywhere. Scheme: yy.m.p
# (26.8.0 = first August-2026 release; p increments within a month,
# so a non-zero p means "this month's p-th patch") with the Android
# versionCode as yymmpp (260800) — monotonic as long as p < 100.
# Both shells always move together: same commit, same version, even
# when only one shell has user-visible changes.
#
# Usage:
#   build-aux/bump-version.sh          # auto: next patch for today
#   build-aux/bump-version.sh 26.8.2   # explicit
#
# Touches:
#   meditate-android/android/app/build.gradle  versionName+Code
#   meditate-android/Cargo.toml                version
#   meditate-gtk/Cargo.toml                    version
#   meditate-gtk/meson.build                   project version
#   build-aux/io.github.janekbt.Meditate.json  APP_VERSION
#   meditate-gtk/data/...metainfo.xml.in       <release> skeleton
#   fastlane .../en-US/changelogs/<code>.txt   skeleton
#
# It does NOT write changelog content, translate, test, tag, or
# push — those are the operator's steps in RELEASE.md.
set -euo pipefail
cd "$(dirname "$0")/.."

GRADLE=meditate-android/android/app/build.gradle
CURRENT=$(grep -oP "versionName '\K[0-9.]+" "$GRADLE")

if [ $# -ge 1 ]; then
    NEW="$1"
else
    YY=$(date +%y); M=$(date +%-m)
    IFS=. read -r cyy cm cp <<< "$CURRENT"
    if [ "$cyy" = "$YY" ] && [ "$cm" = "$M" ]; then
        NEW="$YY.$M.$((cp + 1))"
    else
        # First release of a new month starts at .0 — a non-zero
        # patch number should mean a patch actually happened.
        NEW="$YY.$M.0"
    fi
fi
IFS=. read -r yy m p <<< "$NEW"
CODE=$(printf '%02d%02d%02d' "$yy" "$m" "$p")
TODAY=$(date +%Y-%m-%d)
echo "bump: $CURRENT -> $NEW (versionCode $CODE)"

OLDCODE=$(grep -oP 'versionCode \K[0-9]+' "$GRADLE")
[ "$CODE" -gt "$OLDCODE" ] || { echo "versionCode $CODE !> $OLDCODE" >&2; exit 1; }

sed -i "s/versionCode $OLDCODE/versionCode $CODE/" "$GRADLE"
sed -i "s/versionName '$CURRENT'/versionName '$NEW'/" "$GRADLE"
sed -i "0,/^version = \".*\"/s//version = \"$NEW\"/" meditate-android/Cargo.toml
sed -i "0,/^version = \".*\"/s//version = \"$NEW\"/" meditate-gtk/Cargo.toml
sed -i "0,/version: '[0-9.]*',/s//version: '$NEW',/" meditate-gtk/meson.build
sed -i "s/\"APP_VERSION\": \"[0-9.]*\"/\"APP_VERSION\": \"$NEW\"/" \
    build-aux/io.github.janekbt.Meditate.json

# Metainfo: insert a release skeleton above the newest entry.
# The TODO must be replaced before commit — CI's appstreamcli
# gate will not catch prose, so RELEASE.md step 3 owns it.
META=meditate-gtk/data/io.github.janekbt.Meditate.metainfo.xml.in
python3 - "$META" "$NEW" "$TODAY" <<'PYEOF'
import sys
path, ver, today = sys.argv[1:]
s = open(path).read()
marker = '    <release version="'
assert f'version="{ver}"' not in s, f"{ver} already in metainfo"
skel = (f'    <release version="{ver}" date="{today}">\n'
        f'      <description translate="yes">\n'
        f'        <p>TODO: release notes (see RELEASE.md step 3)</p>\n'
        f'      </description>\n'
        f'    </release>\n')
i = s.index(marker)
open(path, 'w').write(s[:i] + skel + s[i:])
PYEOF

CHANGELOG=fastlane/metadata/android/en-US/changelogs/$CODE.txt
[ -f "$CHANGELOG" ] || echo "TODO: release notes (see RELEASE.md step 3)" > "$CHANGELOG"

echo "done — now follow RELEASE.md from step 3 (changelogs)."
