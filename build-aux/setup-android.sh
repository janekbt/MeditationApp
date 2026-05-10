#!/usr/bin/env bash
# setup-android.sh — bring this Debian / Ubuntu laptop to a state
# where `x run` (xbuild) can build the meditate-android crate for a
# real Android device.
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
# JDK: pick the first package available on the host's apt index, in
# preference order. xbuild and the Android Gradle plugin work with
# any JDK >= 17, so we don't need to pin a single version. Debian 13
# trixie, for example, dropped 17 from its archive — it ships 21 and
# 25. The selection happens at install time, see Step 1.
JDK_CANDIDATES=(
    "openjdk-21-jdk-headless"   # current LTS; default for Debian 13, Ubuntu 24.04+
    "openjdk-17-jdk-headless"   # previous LTS; still on Debian 12, Ubuntu 22.04
    "openjdk-25-jdk-headless"   # latest non-LTS, fallback for very fresh distros
)
PINNED_OPENJDK_PKG=""           # filled in at runtime by Step 1
PINNED_CMDLINE_TOOLS_BUILD="14742923"   # build number, see https://developer.android.com/studio#command-line-tools-only
PINNED_API_LEVEL="35"                    # Android 15
PINNED_BUILD_TOOLS="35.0.0"
PINNED_NDK="27.2.12479018"               # NDK r27c (LTS track)
RUST_ANDROID_TARGETS=(
    "aarch64-linux-android"     # most real devices
    "armv7-linux-androideabi"   # older 32-bit ARM, still common in F-Droid bug reports
    "x86_64-linux-android"      # emulator
)
# xbuild has no crates.io release. Pinning by git is best-effort —
# bump XBUILD_GIT_REV to update. Empty string = use whatever is at
# the default branch tip (less reproducible).
XBUILD_GIT_URL="https://github.com/rust-mobile/xbuild.git"
XBUILD_GIT_REV=""

# ── Paths ────────────────────────────────────────────────────────────
ANDROID_HOME="${HOME}/Android/Sdk"
ANDROID_NDK_ROOT="${ANDROID_HOME}/ndk/${PINNED_NDK}"
# JAVA_HOME is detected from the installed JDK package — see Step 1.
JAVA_HOME=""
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
- OpenJDK headless (whichever of ${JDK_CANDIDATES[*]} apt offers)
- Android command-line tools build ${PINNED_CMDLINE_TOOLS_BUILD}
- Android SDK platform-${PINNED_API_LEVEL}, build-tools ${PINNED_BUILD_TOOLS}, NDK ${PINNED_NDK}
- Rust targets: ${RUST_ANDROID_TARGETS[*]}
- xbuild (cargo install from git: ${XBUILD_GIT_URL})

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

# ── Distro check ─────────────────────────────────────────────────────
if [[ ! -r /etc/os-release ]]; then
    echo "Cannot read /etc/os-release — refusing to guess distro." >&2
    exit 1
fi
. /etc/os-release
if [[ "${ID:-}" != "debian" && "${ID:-}" != "ubuntu" && "${ID_LIKE:-}" != *debian* ]]; then
    echo "This script supports Debian / Ubuntu only. Detected ID=${ID:-unknown}." >&2
    echo "Adapt the apt step to your package manager and rerun." >&2
    exit 1
fi

# ── Helpers ──────────────────────────────────────────────────────────
log() { printf '\033[1;36m[setup-android]\033[0m %s\n' "$*"; }
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

# ── Step 1: OpenJDK ──────────────────────────────────────────────────
# Refresh the apt index once so apt-cache show reflects current
# availability before we pick a JDK candidate.
log "apt: refreshing package index"
sudo apt-get update -y >/dev/null

for cand in "${JDK_CANDIDATES[@]}"; do
    if apt-cache show "${cand}" >/dev/null 2>&1; then
        PINNED_OPENJDK_PKG="${cand}"
        break
    fi
done
if [[ -z "${PINNED_OPENJDK_PKG}" ]]; then
    echo "None of the JDK candidates are available on this host." >&2
    echo "Tried: ${JDK_CANDIDATES[*]}" >&2
    echo "Run 'apt-cache search openjdk-.*-jdk-headless' to see what your distro ships," >&2
    echo "then add the right package to JDK_CANDIDATES at the top of this script." >&2
    exit 1
fi
log "JDK selected: ${PINNED_OPENJDK_PKG}"

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

installed_targets="$(rustup target list --installed)"
for tgt in "${RUST_ANDROID_TARGETS[@]}"; do
    if grep -Fxq "${tgt}" <<<"${installed_targets}"; then
        log "rustup: target ${tgt} already installed"
    else
        log "rustup: adding target ${tgt}"
        rustup target add "${tgt}"
    fi
done

# ── Step 5: xbuild ───────────────────────────────────────────────────
if command -v x >/dev/null 2>&1; then
    log "xbuild (\`x\`) already on PATH at $(command -v x)"
else
    log "Installing xbuild from ${XBUILD_GIT_URL}"
    if [[ -n "${XBUILD_GIT_REV}" ]]; then
        cargo install --git "${XBUILD_GIT_URL}" --rev "${XBUILD_GIT_REV}"
    else
        cargo install --git "${XBUILD_GIT_URL}"
    fi
fi

# ── Step 6: Optional emulator ────────────────────────────────────────
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

# ── Step 7: Env file + bashrc snippet ────────────────────────────────
mkdir -p "$(dirname "${ENV_FILE}")"
cat > "${ENV_FILE}" <<EOF
# Generated by build-aux/setup-android.sh — DO NOT EDIT.
# Bump pins in setup-android.sh and re-run.
export JAVA_HOME="${JAVA_HOME}"
export ANDROID_HOME="${ANDROID_HOME}"
export ANDROID_SDK_ROOT="\${ANDROID_HOME}"   # legacy alias some tools still read
export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT}"
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

# ── Step 8: Smoke test ───────────────────────────────────────────────
# Source the env so this same shell sees the new PATH for the smoke.
. "${ENV_FILE}"

log "Smoke test:"
printf '  java     : '; java -version 2>&1 | head -1
printf '  sdkmanager: '; "${SDKMANAGER}" --version
printf '  adb      : '; adb --version | head -1
printf '  ndk      : '; head -1 "${ANDROID_NDK_ROOT}/source.properties"
printf '  rustup targets:\n'; rustup target list --installed | grep linux-android | sed 's/^/    - /'
printf '  xbuild   : '; x --version 2>&1 || echo "not on PATH yet — open a new shell"

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
