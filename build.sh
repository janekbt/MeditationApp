#!/usr/bin/env bash
# build.sh — hands-off build-from-source for both shells.
#
#   ./build.sh gtk                # GTK app, release build
#   ./build.sh android            # optimized APK (debug-keystore-
#                                 # signed; F-Droid re-signs, fine
#                                 # for sideloading)
#   ./build.sh {gtk|android} --debug   # faster build for hacking
#
# What "hands-off" means here: dependency installation for
# Debian/Ubuntu, Fedora and Arch (sudo will prompt once), version
# floors checked BEFORE the long compile, known pitfalls handled:
#   * GTK/libadwaita floor (4.18 / 1.7) — checked up front with a
#     clear message + the Flatpak fallback, instead of a mid-build
#     pkg-config error.
#   * blueprint-compiler >= 0.20 required (Debian bookworm's is
#     too old — the message says what to do).
#   * Rust via rustup preferred; the Android target install needs
#     rustup, and distro rustc that is too old fails cargo's
#     edition check mid-build otherwise.
#   * Low-RAM guard: heavy LTO link steps have frozen a 16 GiB
#     machine before — parallelism is capped when RAM is tight.
#   * Android toolchain (SDK/NDK/Gradle) is bootstrapped by
#     build-aux/setup-android.sh (idempotent, pinned versions); on
#     non-Debian distros this script pre-installs the three
#     packages that bootstrap needs (JDK/unzip/wget).
set -euo pipefail
cd "$(dirname "$0")"

log()  { printf '\033[1;32m[build]\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m[build]\033[0m %s\n' "$*" >&2; exit 1; }

TARGET="${1:-}"; shift || true
MODE=release
for a in "$@"; do case "$a" in
    --debug) MODE=debug ;;
    --release) MODE=release ;;
    *) die "unknown flag: $a" ;;
esac; done
[[ "$TARGET" == gtk || "$TARGET" == android ]] \
    || die "usage: ./build.sh {gtk|android} [--debug|--release]"

# ── Distro detection ─────────────────────────────────────────────────
PM=""
if command -v apt-get >/dev/null; then PM=apt
elif command -v dnf >/dev/null;   then PM=dnf
elif command -v pacman >/dev/null; then PM=pacman
fi

install_pkgs() {
    # $@ = packages in the current PM's naming. No-op if PM unknown
    # (caller printed the manual list already).
    case "$PM" in
        apt)    sudo apt-get update -qq
                sudo apt-get install -y --no-install-recommends "$@" ;;
        dnf)    sudo dnf install -y "$@" ;;
        pacman) sudo pacman -S --needed --noconfirm "$@" ;;
    esac
}

# ── Shared: Rust ─────────────────────────────────────────────────────
ensure_rust() {
    if command -v rustup >/dev/null; then
        return
    fi
    if command -v cargo >/dev/null && [[ "$TARGET" == gtk ]]; then
        # Distro cargo can build the GTK shell if new enough
        # (rust-toolchain.toml is a rustup concept; plain cargo
        # ignores it). Warn if clearly ancient.
        local v; v=$(rustc --version | grep -oP '1\.\K[0-9]+')
        (( v >= 85 )) || die "rustc 1.$v is too old — install rustup: https://rustup.rs"
        log "using distro rust ($(rustc --version)) — fine for the GTK build"
        return
    fi
    log "installing rustup (official installer, ~/.cargo) …"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path
    export PATH="$HOME/.cargo/bin:$PATH"
}

# ── Shared: RAM guard ────────────────────────────────────────────────
ram_guard() {
    local kb; kb=$(grep MemTotal /proc/meminfo | grep -oP '[0-9]+')
    if (( kb < 20000000 )); then
        export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
        log "RAM < 20 GiB — capping cargo at ${CARGO_BUILD_JOBS} jobs (heavy LTO links have frozen smaller machines)"
    fi
}

# ═════════════════════════════════════ GTK ═══════════════════════════
build_gtk() {
    log "target: GTK shell ($MODE)"
    # Skip the package step entirely when everything is already
    # there — repeat builds shouldn't need sudo at all.
    if pkg-config --exists gtk4 libadwaita-1 gstreamer-1.0 2>/dev/null \
        && command -v meson >/dev/null && command -v ninja >/dev/null \
        && command -v blueprint-compiler >/dev/null \
        && command -v msgfmt >/dev/null; then
        log "system dependencies present — skipping package installation"
    else
    case "$PM" in
        apt)    install_pkgs build-essential meson ninja-build pkg-config \
                    libgtk-4-dev libadwaita-1-dev libgstreamer1.0-dev \
                    libgstreamer-plugins-base1.0-dev blueprint-compiler \
                    desktop-file-utils gettext ;;
        dnf)    install_pkgs gcc meson ninja-build pkgconf-pkg-config \
                    gtk4-devel libadwaita-devel gstreamer1-devel \
                    gstreamer1-plugins-base-devel blueprint-compiler \
                    desktop-file-utils gettext ;;
        pacman) install_pkgs base-devel meson ninja pkgconf gtk4 \
                    libadwaita gstreamer gst-plugins-base \
                    blueprint-compiler desktop-file-utils gettext ;;
        *)  log "unknown package manager — make sure these exist:"
            log "  meson ninja pkg-config gtk4>=4.18 libadwaita>=1.7 gstreamer blueprint-compiler>=0.20 gettext rust" ;;
    esac
    fi

    # Fail-early floors (the #1 support question: distro GTK too old).
    pkg-config --atleast-version=4.18 gtk4 \
        || die "gtk4 $(pkg-config --modversion gtk4 2>/dev/null || echo '?') is below the 4.18 floor.
Your distro is too old for a native build — use the Flatpak path instead:
  flatpak-builder --user --install --force-clean --install-deps-from=flathub \\
      flatpak_app build-aux/io.github.janekbt.Meditate.json"
    pkg-config --atleast-version=1.7 libadwaita-1 \
        || die "libadwaita $(pkg-config --modversion libadwaita-1 2>/dev/null || echo '?') is below the 1.7 floor — see the Flatpak fallback above."
    bp=$(blueprint-compiler --version 2>/dev/null | grep -oP '[0-9]+\.[0-9]+' | head -1 || echo 0)
    awk "BEGIN{exit !($bp >= 0.16)}" \
        || die "blueprint-compiler $bp is below 0.16 — install a newer one (pipx install blueprint-compiler works too)."

    ensure_rust
    ram_guard

    local bt=release; [[ "$MODE" == debug ]] && bt=debug
    ( cd meditate-gtk
      if [[ ! -d builddir ]]; then
          meson setup builddir --buildtype="$bt"
      else
          meson configure builddir --buildtype="$bt" >/dev/null
      fi
      ninja -C builddir )
    log "done → meditate-gtk/builddir/src/meditate"
    log "run it:            ./meditate-gtk/builddir/src/meditate"
    log "install system-wide: sudo ninja -C meditate-gtk/builddir install"
}

# ═══════════════════════════════════ Android ═════════════════════════
build_android() {
    log "target: Android APK ($MODE)"
    # setup-android.sh needs exactly javac/unzip/wget from the
    # distro; it direct-downloads everything else (SDK, NDK r27,
    # Gradle, kotlinc) with pinned versions, idempotently.
    if ! { command -v javac && command -v unzip && command -v wget; } >/dev/null 2>&1; then
        case "$PM" in
            apt)    : ;;  # setup-android.sh handles apt itself
            dnf)    install_pkgs java-21-openjdk-devel unzip wget ;;
            pacman) install_pkgs jdk21-openjdk unzip wget ;;
            *)      die "install a JDK (17+), unzip and wget, then rerun." ;;
        esac
    fi
    ENV_SH="$HOME/.config/meditate-android/env.sh"
    if [[ -r "$ENV_SH" ]] && . "$ENV_SH" 2>/dev/null \
        && [[ -d "${ANDROID_NDK_ROOT:-/nonexistent}" ]]; then
        log "Android toolchain present (env.sh) — skipping bootstrap"
    else
        log "bootstrapping Android SDK/NDK (one-time, ~3 GiB download) …"
        build-aux/setup-android.sh
        . "$ENV_SH"
    fi

    ensure_rust
    command -v rustup >/dev/null \
        || die "the Android target needs rustup (distro cargo can't add cross targets) — https://rustup.rs"
    rustup target add aarch64-linux-android >/dev/null
    ram_guard

    local task=assembleDebug apk=debug/app-debug.apk
    [[ "$MODE" == release ]] && { task=assembleRelease; apk=release/app-release.apk; }
    ( cd meditate-android/android && ./gradlew ":app:${task}" )
    local out="meditate-android/android/app/build/outputs/apk/${apk}"
    [[ -f "$out" ]] || die "gradle reported success but ${out} is missing"
    log "done → ${out}"
    log "install: adb install -r ${out}"
    if [[ "$MODE" == release ]]; then
        log "note: release APKs are debug-keystore-signed (F-Droid re-signs; fine for sideloading)"
    fi
}

case "$TARGET" in
    gtk)     build_gtk ;;
    android) build_android ;;
esac
