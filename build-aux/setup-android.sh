#!/usr/bin/env bash
# setup-android.sh — bring a Debian / Ubuntu machine to a state
# where the meditate-android Gradle pipeline can build for a real
# Android device (writes ~/.config/meditate-android/env.sh, which
# meditate-android/android/rust-build.sh and the gradle wrapper
# invocation both source).
#
# Idempotent: every step gates on a presence check, so re-running is
# a near no-op once the toolchain is in place. No Android Studio,
# no emulator image by default. Pass --with-emulator to add an AVD.
#
# Pinned versions live at the top — change there, not below.
#
# Sister script: build-aux/dev-xbuild.sh (Linux/aarch64 cross-build
# for the Librem 5). They share no state.
set -euo pipefail

# Mirror everything (stdout + stderr) to a log file so we can diagnose
# after the fact even if the terminal window closes on exit. The tee
# subprocess runs concurrently with the script and survives the
# script's exit long enough to flush.
LOG_FILE="${TMPDIR:-/tmp}/setup-android.log"
exec > >(tee "${LOG_FILE}") 2>&1

# Pause on any failure so the user can read the error before the
# terminal closes. Many GUI terminal launchers close the window the
# moment the foreground process exits, which swallows the diagnostic
# line that would tell us what went wrong. The log file is the
# fallback when the prompt gets eaten.
trap '{
    rc=$?
    echo
    echo "[setup-android] Failed with exit code ${rc}. See the message above this line." >&2
    echo "[setup-android] Full log: ${LOG_FILE}" >&2
    if [[ -t 0 ]]; then
        read -rp "Press Enter to close..." _ || true
    fi
}' ERR

# ── Pinned versions ──────────────────────────────────────────────────
# JDK 17, exactly: rust-build.sh pins the d8 dexer from build-tools
# 31.0.0 (to match F-Droid's buildserver for reproducible builds), and
# that d8 crashes on classes compiled by a newer javac ("Dex conversion
# failed"). F-Droid's recipe installs Debian 12's openjdk-17; Debian 13
# no longer ships 17 — Step 1 prints the three commands to add it.
PINNED_JDK_MAJOR="17"
PINNED_OPENJDK_PKG="openjdk-17-jdk-headless"
PINNED_CMDLINE_TOOLS_BUILD="14742923"   # build number, see https://developer.android.com/studio#command-line-tools-only
PINNED_API_LEVEL="35"                    # Android 15 — emulator image; build-tools 35 puts apksigner on PATH
PINNED_BUILD_TOOLS="35.0.0"
# Reproducible-release pins: MUST match rust-build.sh (ANDROID_JAR →
# platform 34, ANDROID_D8_JAR → build-tools 31.0.0) and app/build.gradle
# (compileSdk 34). Without them rust-build.sh still builds, but the dex
# no longer matches F-Droid's rebuild.
PINNED_REPRO_PLATFORM="34"
PINNED_REPRO_BUILD_TOOLS="31.0.0"
PINNED_NDK="27.2.12479018"               # NDK r27c (LTS track)
RUST_ANDROID_TARGETS=(
    "aarch64-linux-android"     # most real devices
    "armv7-linux-androideabi"   # older 32-bit ARM, still common in F-Droid bug reports
    "x86_64-linux-android"      # emulator
)

# Standalone Gradle + Kotlin compiler for manual use. `./gradlew` does
# not need them (the committed wrapper fetches Gradle 8.5 and the
# Kotlin Gradle plugin brings its own compiler); Gradle 8.5 matches the
# wrapper, the ceiling for the AGP 7.3 the project pins.
PINNED_GRADLE="8.5"
PINNED_KOTLIN="1.9.25"

# ── Paths ────────────────────────────────────────────────────────────
ANDROID_HOME="${HOME}/Android/Sdk"
ANDROID_NDK_ROOT="${ANDROID_HOME}/ndk/${PINNED_NDK}"
GRADLE_HOME="${HOME}/Android/gradle-${PINNED_GRADLE}"
KOTLIN_HOME="${HOME}/Android/kotlinc-${PINNED_KOTLIN}"
# JAVA_HOME is set to the JDK 17 home — see Prerequisites / Step 1.
JAVA_HOME=""
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${HOME}/.config/meditate-android/env.sh"
BASHRC="${HOME}/.bashrc"
BASHRC_MARKER_BEGIN="# >>> meditate-android setup >>>"
BASHRC_MARKER_END="# <<< meditate-android setup <<<"

WITH_EMULATOR=0

# ── Usage ────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Usage: $(basename "$0") [--with-emulator] [--help]

Installs the Android toolchain needed by the meditate-android crate:
- OpenJDK ${PINNED_JDK_MAJOR} (${PINNED_OPENJDK_PKG}; on Debian 13 add the bookworm source first — the script says how)
- Android command-line tools build ${PINNED_CMDLINE_TOOLS_BUILD}
- Android SDK platform-${PINNED_REPRO_PLATFORM} + build-tools ${PINNED_REPRO_BUILD_TOOLS} (reproducible release pins),
  platform-${PINNED_API_LEVEL}, build-tools ${PINNED_BUILD_TOOLS}, NDK ${PINNED_NDK}
- Rust targets for the repo's pinned toolchain: ${RUST_ANDROID_TARGETS[*]}
- Gradle ${PINNED_GRADLE} (binary distribution from gradle.org)
- Kotlin ${PINNED_KOTLIN} compiler (binary distribution from JetBrains)

Writes:
- ${ENV_FILE}                   (env vars, owned by this script)
- ${BASHRC}                     (one-time marker-bracketed source line)

Flags:
  --with-emulator   Also install system-images;android-${PINNED_API_LEVEL};google_apis;x86_64
                    and create an AVD named "meditate-test". Off by default
                    because the emulator + system image is several GiB and
                    this laptop has limited swap headroom.
  --help            This message.

Re-running the script is a near no-op once installed. Bump the
PINNED_* values at the top of the file and re-run to upgrade.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --with-emulator) WITH_EMULATOR=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "Unknown flag: $1" >&2; usage; exit 2 ;;
    esac
done

# ── Helpers ──────────────────────────────────────────────────────────
log() { printf '\033[1;36m[setup-android]\033[0m %s\n' "$*"; }

# ── Prerequisites ────────────────────────────────────────────────────
# apt only ever provides three things (JDK 17, unzip, wget); the SDK,
# NDK, Gradle and kotlinc are direct downloads. If all three are
# already present (any distro — build.sh pre-installs them on
# Fedora/Arch), skip the apt machinery entirely.

# Print the JDK 17 home, or nothing. A JDK other than 17 on PATH does
# not count (see PINNED_JDK_MAJOR).
find_jdk17() {
    local c
    for c in /usr/lib/jvm/java-17-openjdk-amd64 /usr/lib/jvm/java-17-openjdk \
             /usr/lib/jvm/java-17-openjdk-* /usr/lib/jvm/java-17-*; do
        if [[ -x "${c}/bin/javac" ]]; then echo "${c}"; return; fi
    done
    if command -v javac >/dev/null 2>&1 \
       && javac -version 2>&1 | grep -q "^javac ${PINNED_JDK_MAJOR}\."; then
        local javac_real; javac_real="$(readlink -f "$(command -v javac)")"
        echo "${javac_real%/bin/javac}"
    fi
}

PREREQS_PRESENT=0
JAVA_HOME="$(find_jdk17)"
if [[ -n "${JAVA_HOME}" ]] \
   && command -v unzip >/dev/null 2>&1 \
   && command -v wget >/dev/null 2>&1; then
    PREREQS_PRESENT=1
    log "prereqs present (JDK ${PINNED_JDK_MAJOR}/unzip/wget) — skipping apt; JDK home: ${JAVA_HOME}"
fi

if [[ "${PREREQS_PRESENT}" = 0 ]]; then
# ── Distro check ─────────────────────────────────────────────────────
if [[ ! -r /etc/os-release ]]; then
    echo "Cannot read /etc/os-release — refusing to guess distro." >&2
    exit 1
fi
. /etc/os-release
if [[ "${ID:-}" != "debian" && "${ID:-}" != "ubuntu" && "${ID_LIKE:-}" != *debian* ]]; then
    echo "This script supports Debian / Ubuntu only (or any distro with" >&2
    echo "JDK ${PINNED_JDK_MAJOR}, unzip and wget preinstalled — e.g. via build.sh)." >&2
    echo "Detected ID=${ID:-unknown}." >&2
    exit 1
fi

need_apt() {
    local pkg="$1"
    if dpkg -s "${pkg}" >/dev/null 2>&1; then
        log "apt: ${pkg} already installed"
    else
        log "apt: installing ${pkg}"
        sudo apt-get update -y
        sudo apt-get install -y --no-install-recommends "${pkg}"
    fi
}

# ── Step 1: OpenJDK 17 ───────────────────────────────────────────────
# Refresh the apt index once so apt-cache show reflects current
# availability.
log "apt: refreshing package index"
sudo apt-get update -y >/dev/null

if ! apt-cache show "${PINNED_OPENJDK_PKG}" >/dev/null 2>&1; then
    cat >&2 <<MSG
${PINNED_OPENJDK_PKG} is not available from your apt sources (Debian 13
dropped it). Add Debian 12's archive for it — the same thing F-Droid's
build recipe does — pinned low so nothing else is ever taken from it:

  echo "deb https://deb.debian.org/debian bookworm main" | sudo tee /etc/apt/sources.list.d/bookworm.list
  printf 'Package: *\nPin: release n=bookworm\nPin-Priority: 100\n' | sudo tee /etc/apt/preferences.d/bookworm-low
  sudo apt update && sudo apt install ${PINNED_OPENJDK_PKG}

then re-run this script.
MSG
    exit 1
fi
log "JDK: ${PINNED_OPENJDK_PKG}"

need_apt "${PINNED_OPENJDK_PKG}"
need_apt "unzip"
need_apt "wget"

# Locate JAVA_HOME from the JDK package we just installed. dpkg -L
# is authoritative — it lists the package's owned paths regardless of
# whether the user has multiple JDKs installed or how their
# update-alternatives setup is configured. We pick `javac` (vs `java`)
# because the headless JDK package owns it; the JRE-headless package
# owns `java`, and we want the JDK home, not the JRE home.
javac_path="$(dpkg -L "${PINNED_OPENJDK_PKG}" 2>/dev/null | grep -E '/bin/javac$' | head -1 || true)"
if [[ -z "${javac_path}" || ! -x "${javac_path}" ]]; then
    echo "Couldn't locate javac after installing ${PINNED_OPENJDK_PKG}." >&2
    echo "Run 'dpkg -L ${PINNED_OPENJDK_PKG} | grep bin/javac' to debug." >&2
    exit 1
fi
JAVA_HOME="${javac_path%/bin/javac}"
log "JDK home: ${JAVA_HOME}"
fi  # PREREQS_PRESENT

# ── Step 2: Android command-line tools ───────────────────────────────
SDKMANAGER="${ANDROID_HOME}/cmdline-tools/latest/bin/sdkmanager"
if [[ -x "${SDKMANAGER}" ]]; then
    log "cmdline-tools already at ${ANDROID_HOME}/cmdline-tools/latest"
else
    log "Downloading Android command-line tools build ${PINNED_CMDLINE_TOOLS_BUILD}"
    mkdir -p "${ANDROID_HOME}/cmdline-tools"
    tmpzip="$(mktemp --suffix=.zip)"
    trap 'rm -f "${tmpzip}"' EXIT
    wget -q --show-progress -O "${tmpzip}" \
        "https://dl.google.com/android/repository/commandlinetools-linux-${PINNED_CMDLINE_TOOLS_BUILD}_latest.zip"
    tmpdir="$(mktemp -d)"
    unzip -q "${tmpzip}" -d "${tmpdir}"
    # Inside the zip, the layout is `cmdline-tools/<files>` — Google
    # expects it at `${ANDROID_HOME}/cmdline-tools/latest/<files>`.
    mv "${tmpdir}/cmdline-tools" "${ANDROID_HOME}/cmdline-tools/latest"
    rm -rf "${tmpdir}"
    rm -f "${tmpzip}"
    trap - EXIT
fi

# Accept SDK licenses up-front; sdkmanager prompts otherwise. The
# `yes` pipe is the documented way (Google's sample CI scripts use it),
# but it interacts badly with `set -o pipefail`: when sdkmanager has
# accepted all licenses and closes stdin, `yes` is killed by SIGPIPE
# and exits 141, which pipefail then propagates as the pipeline's
# exit code even though sdkmanager itself succeeded. Disable pipefail
# for this one pipeline and check sdkmanager's exit code explicitly.
log "Accepting SDK licenses"
set +o pipefail
yes 2>/dev/null | "${SDKMANAGER}" --sdk_root="${ANDROID_HOME}" --licenses >/dev/null
licenses_rc=${PIPESTATUS[1]}
set -o pipefail
if [[ ${licenses_rc} -ne 0 ]]; then
    echo "sdkmanager --licenses failed (exit ${licenses_rc})" >&2
    exit "${licenses_rc}"
fi

# ── Step 3: SDK platform, build-tools, platform-tools, NDK ───────────
sdk_install_if_missing() {
    local pkg="$1"
    if "${SDKMANAGER}" --sdk_root="${ANDROID_HOME}" --list_installed 2>/dev/null \
            | awk '{print $1}' | grep -Fxq "${pkg}"; then
        log "sdk: ${pkg} already installed"
    else
        log "sdk: installing ${pkg}"
        "${SDKMANAGER}" --sdk_root="${ANDROID_HOME}" --install "${pkg}"
    fi
}

sdk_install_if_missing "platform-tools"
sdk_install_if_missing "platforms;android-${PINNED_REPRO_PLATFORM}"
sdk_install_if_missing "build-tools;${PINNED_REPRO_BUILD_TOOLS}"
sdk_install_if_missing "platforms;android-${PINNED_API_LEVEL}"
sdk_install_if_missing "build-tools;${PINNED_BUILD_TOOLS}"
sdk_install_if_missing "ndk;${PINNED_NDK}"

if [[ ! -d "${ANDROID_NDK_ROOT}" ]]; then
    echo "NDK missing at ${ANDROID_NDK_ROOT} despite sdkmanager success — bailing." >&2
    exit 1
fi

# ── Step 4: Rust targets ─────────────────────────────────────────────
# rustup may already be on PATH (interactive shells source ~/.cargo/env
# from .bashrc), but a script launched without a login/interactive
# shell init can have a thinner PATH. Fall back to the canonical install
# locations before giving up.
if ! command -v rustup >/dev/null 2>&1; then
    if [[ -f "${HOME}/.cargo/env" ]]; then
        # shellcheck disable=SC1091
        . "${HOME}/.cargo/env"
    fi
fi
if ! command -v rustup >/dev/null 2>&1 && [[ -x "${HOME}/.cargo/bin/rustup" ]]; then
    export PATH="${HOME}/.cargo/bin:${PATH}"
fi
if ! command -v rustup >/dev/null 2>&1; then
    echo "rustup not found in PATH or under ~/.cargo/bin." >&2
    echo "Install rustup first: https://rustup.rs/" >&2
    exit 1
fi

# Run inside the repo so the targets land on the toolchain pinned by
# rust-toolchain.toml (the one every build uses), not rustup's default.
installed_targets="$(cd "${REPO_ROOT}" && rustup target list --installed)"
for tgt in "${RUST_ANDROID_TARGETS[@]}"; do
    if grep -Fxq "${tgt}" <<<"${installed_targets}"; then
        log "rustup: target ${tgt} already installed"
    else
        log "rustup: adding target ${tgt}"
        (cd "${REPO_ROOT}" && rustup target add "${tgt}")
    fi
done

# ── Step 5: (none) ───────────────────────────────────────────────────
# The APK is built by the committed Gradle project at
# meditate-android/android/; its cargoNdkBuild task runs rust-build.sh
# (plain `cargo build --target …` with the NDK linker/CC/AR exported —
# NOT cargo-ndk, whose 4.x runner breaks slint's build.rs SDK lookup).

# ── Step 6: Gradle ───────────────────────────────────────────────────
# Debian's apt `gradle` package lags upstream by years (Debian 13
# ships a 4.x), so we install the official binary distribution
# straight from gradle.org. The download is ~150 MB; idempotent via
# the presence check below.
if [[ -x "${GRADLE_HOME}/bin/gradle" ]]; then
    log "gradle already at ${GRADLE_HOME}"
else
    log "Downloading Gradle ${PINNED_GRADLE}"
    mkdir -p "$(dirname "${GRADLE_HOME}")"
    tmpzip="$(mktemp --suffix=.zip)"
    trap 'rm -f "${tmpzip}"' EXIT
    wget -q --show-progress -O "${tmpzip}" \
        "https://services.gradle.org/distributions/gradle-${PINNED_GRADLE}-bin.zip"
    # The zip unpacks as `gradle-<version>/...`, which matches our
    # ${GRADLE_HOME} path layout (one tree per pinned version, so
    # bumping the pin doesn't leave the previous install behind).
    unzip -q "${tmpzip}" -d "$(dirname "${GRADLE_HOME}")"
    rm -f "${tmpzip}"
    trap - EXIT
    if [[ ! -x "${GRADLE_HOME}/bin/gradle" ]]; then
        echo "Gradle install at ${GRADLE_HOME} doesn't have bin/gradle — bailing." >&2
        exit 1
    fi
fi

# ── Step 7: Kotlin compiler ──────────────────────────────────────────
# JetBrains' standalone kotlin-compiler release, for manual use (see
# PINNED_KOTLIN). Same idempotent pattern as cmdline-tools above.
if [[ -x "${KOTLIN_HOME}/bin/kotlinc" ]]; then
    log "kotlin already at ${KOTLIN_HOME}"
else
    log "Downloading Kotlin compiler ${PINNED_KOTLIN}"
    mkdir -p "$(dirname "${KOTLIN_HOME}")"
    tmpzip="$(mktemp --suffix=.zip)"
    trap 'rm -f "${tmpzip}"' EXIT
    wget -q --show-progress -O "${tmpzip}" \
        "https://github.com/JetBrains/kotlin/releases/download/v${PINNED_KOTLIN}/kotlin-compiler-${PINNED_KOTLIN}.zip"
    # The zip unpacks as `kotlinc/...`; rename to the versioned path
    # so future version bumps don't collide.
    tmpdir="$(mktemp -d)"
    unzip -q "${tmpzip}" -d "${tmpdir}"
    mv "${tmpdir}/kotlinc" "${KOTLIN_HOME}"
    rm -rf "${tmpdir}"
    rm -f "${tmpzip}"
    trap - EXIT
    if [[ ! -x "${KOTLIN_HOME}/bin/kotlinc" ]]; then
        echo "Kotlin install at ${KOTLIN_HOME} doesn't have bin/kotlinc — bailing." >&2
        exit 1
    fi
fi

# ── Step 8: Optional emulator ────────────────────────────────────────
if [[ "${WITH_EMULATOR}" -eq 1 ]]; then
    sdk_install_if_missing "emulator"
    sdk_install_if_missing "system-images;android-${PINNED_API_LEVEL};google_apis;x86_64"

    AVDMANAGER="${ANDROID_HOME}/cmdline-tools/latest/bin/avdmanager"
    if "${AVDMANAGER}" list avd 2>/dev/null | grep -q "Name: meditate-test"; then
        log "emulator: AVD 'meditate-test' already exists"
    else
        log "emulator: creating AVD 'meditate-test'"
        echo "no" | "${AVDMANAGER}" create avd \
            --force \
            --name "meditate-test" \
            --package "system-images;android-${PINNED_API_LEVEL};google_apis;x86_64"
    fi
fi

# ── Step 9: Env file + bashrc snippet ────────────────────────────────
mkdir -p "$(dirname "${ENV_FILE}")"
cat > "${ENV_FILE}" <<EOF
# Generated by build-aux/setup-android.sh — DO NOT EDIT.
# Bump pins in setup-android.sh and re-run.
export JAVA_HOME="\${JAVA_HOME:-${JAVA_HOME}}"
export ANDROID_HOME="${ANDROID_HOME}"
export ANDROID_SDK_ROOT="\${ANDROID_HOME}"   # legacy alias some tools still read
export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT}"
export GRADLE_HOME="${GRADLE_HOME}"
export KOTLIN_HOME="${KOTLIN_HOME}"
case ":\${PATH}:" in
    *":\${JAVA_HOME}/bin:"*) ;;
    *) export PATH="\${JAVA_HOME}/bin:\${PATH}" ;;
esac
case ":\${PATH}:" in
    *":\${ANDROID_HOME}/cmdline-tools/latest/bin:"*) ;;
    *) export PATH="\${ANDROID_HOME}/cmdline-tools/latest/bin:\${PATH}" ;;
esac
case ":\${PATH}:" in
    *":\${ANDROID_HOME}/platform-tools:"*) ;;
    *) export PATH="\${ANDROID_HOME}/platform-tools:\${PATH}" ;;
esac
case ":\${PATH}:" in
    *":\${ANDROID_HOME}/build-tools/${PINNED_BUILD_TOOLS}:"*) ;;
    *) export PATH="\${ANDROID_HOME}/build-tools/${PINNED_BUILD_TOOLS}:\${PATH}" ;;
esac
# NDK's clang toolchain on PATH. rust-build.sh references the
# per-ABI clang/llvm-ar by absolute path, but keeping the NDK bin
# on PATH also lets the cc-rs build scripts of C deps (ring,
# skia-bindings, libsqlite3-sys) find the right tools by name.
case ":\${PATH}:" in
    *":\${ANDROID_NDK_ROOT}/toolchains/llvm/prebuilt/linux-x86_64/bin:"*) ;;
    *) export PATH="\${ANDROID_NDK_ROOT}/toolchains/llvm/prebuilt/linux-x86_64/bin:\${PATH}" ;;
esac
# Standalone Gradle + Kotlin compiler for manual use (./gradlew brings
# its own).
case ":\${PATH}:" in
    *":\${GRADLE_HOME}/bin:"*) ;;
    *) export PATH="\${GRADLE_HOME}/bin:\${PATH}" ;;
esac
case ":\${PATH}:" in
    *":\${KOTLIN_HOME}/bin:"*) ;;
    *) export PATH="\${KOTLIN_HOME}/bin:\${PATH}" ;;
esac
EOF
log "Wrote ${ENV_FILE}"

# Append a marker-bracketed source line to ~/.bashrc, replacing any
# previous block to stay idempotent across version bumps.
if [[ -f "${BASHRC}" ]] && grep -Fq "${BASHRC_MARKER_BEGIN}" "${BASHRC}"; then
    # Strip existing block, then re-append the current one.
    sed -i "/${BASHRC_MARKER_BEGIN}/,/${BASHRC_MARKER_END}/d" "${BASHRC}"
fi
cat >> "${BASHRC}" <<EOF
${BASHRC_MARKER_BEGIN}
[ -f "${ENV_FILE}" ] && . "${ENV_FILE}"
${BASHRC_MARKER_END}
EOF
log "Updated ${BASHRC} (sources ${ENV_FILE})"

# ── Step 10: Smoke test ──────────────────────────────────────────────
# Source the env so this same shell sees the new PATH for the smoke.
. "${ENV_FILE}"

log "Smoke test:"
printf '  java     : '; java -version 2>&1 | head -1
printf '  sdkmanager: '; "${SDKMANAGER}" --version
printf '  adb      : '; adb --version | head -1
printf '  ndk      : '; head -1 "${ANDROID_NDK_ROOT}/source.properties"
printf '  rustup targets (pinned toolchain):\n'; (cd "${REPO_ROOT}" && rustup target list --installed) | grep linux-android | sed 's/^/    - /'
printf '  gradle   : '; gradle --version 2>/dev/null | grep '^Gradle' || echo "not on PATH yet — open a new shell"
printf '  kotlinc  : '; kotlinc -version 2>&1 | head -1 || echo "not on PATH yet — open a new shell"

echo
echo "════════════════════════════════════════════════════════════════════"
echo " Everything installed. Open a new shell or run"
echo "   . ${ENV_FILE}"
echo " to pick up the new env vars in this session."
echo " Full transcript: ${LOG_FILE}"
echo "════════════════════════════════════════════════════════════════════"
if [[ -t 0 ]]; then
    read -rp "Press Enter to close the window..." _ || true
fi
