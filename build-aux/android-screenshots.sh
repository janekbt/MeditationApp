#!/usr/bin/env bash
# Recapture the Android store screenshot of the main screen in every
# fastlane locale: one light and one dark capture each, joined along
# the diagonal by compose-screenshot.py into
# fastlane/metadata/android/<locale>/images/phoneScreenshots/1-timer.png.
#
#   build-aux/android-screenshots.sh [adb-serial]
#
# The phone must be unlocked with Meditate installed. No taps: each
# capture relaunches the app on its main screen, and the script stops
# if Meditate isn't the focused window. The phone's theme and the app
# language are put back at the end, also on Ctrl+C or an error.
set -euo pipefail

PKG=io.github.janekbt.Meditate
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ADB=(adb ${1:+-s "$1"})
OUT="$(mktemp -d)"

# fastlane folder -> Android locale tag
declare -A LOCALES=(
  [en-US]=en-US [de-DE]=de [es-ES]=es [fr-FR]=fr [it-IT]=it
  [nl-NL]=nl [pl-PL]=pl [pt-BR]=pt-BR [ru-RU]=ru [zh-CN]=zh-CN
)

night_before="$("${ADB[@]}" shell cmd uimode night | sed 's/.*: //' | tr -d '\r')"
restore() {
  "${ADB[@]}" shell cmd uimode night "$night_before" >/dev/null || true
  "${ADB[@]}" shell cmd locale set-app-locales "$PKG" >/dev/null || true
  echo "Phone theme ($night_before) and app language restored."
}
trap restore EXIT

capture() { # locale-tag file
  "${ADB[@]}" shell cmd locale set-app-locales "$PKG" --locales "$1"
  # A relaunch, not a recreate: the recreate races the locale switch.
  "${ADB[@]}" shell am force-stop "$PKG"
  "${ADB[@]}" shell am start -W -n "$PKG/android.app.NativeActivity" >/dev/null
  sleep 3
  if ! "${ADB[@]}" shell dumpsys window | grep -q "mCurrentFocus=.*$PKG"; then
    echo "Meditate is not in front, stopping." >&2
    exit 1
  fi
  "${ADB[@]}" exec-out screencap -p >"$2"
}

for theme in no yes; do
  "${ADB[@]}" shell cmd uimode night "$theme" >/dev/null
  sleep 2
  for folder in "${!LOCALES[@]}"; do
    echo "capture $folder ($([ $theme = yes ] && echo dark || echo light))"
    capture "${LOCALES[$folder]}" "$OUT/$folder-$theme.png"
  done
done

for folder in "${!LOCALES[@]}"; do
  python3 -I "$ROOT/build-aux/compose-screenshot.py" "$OUT/$folder-no.png" "$OUT/$folder-yes.png" \
    "$ROOT/fastlane/metadata/android/$folder/images/phoneScreenshots/1-timer.png"
done

# A locale that leaked shows the same bottom navigation as another one.
dupes="$(for f in "$OUT"/*-no.png; do
  python3 -I -c 'import sys,hashlib; from PIL import Image
im = Image.open(sys.argv[1]); w, h = im.size
print(hashlib.sha1(im.crop((0, int(h * .88), w, int(h * .97))).tobytes()).hexdigest(), sys.argv[1])' "$f"
done | sort | uniq -D -w 40)"
if [ -n "$dupes" ]; then
  echo "Same navigation strip in more than one locale (a language leaked?):" >&2
  echo "$dupes" >&2
fi
echo "Raw captures in $OUT"
