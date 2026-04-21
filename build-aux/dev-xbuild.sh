#!/usr/bin/env bash
# Cross-compile the meditate binary for the Librem 5 (aarch64) directly
# on this x86_64 laptop, without the ~35-minute flatpak-builder QEMU tax.
#
# Prereqs (one-time):
#   rustup target add aarch64-unknown-linux-gnu
#   sudo apt install gcc-aarch64-linux-gnu
#   flatpak install --user --arch=aarch64 flathub org.gnome.Sdk//50
#   ln -sfn ~/.local/share/flatpak/runtime/org.gnome.Sdk/aarch64/50/active/files \
#          ~/sysroots/gnome50-aarch64/usr
#
# Output: target/aarch64-unknown-linux-gnu/release/meditate
set -euo pipefail

SYSROOT="${HOME}/sysroots/gnome50-aarch64"
if [[ ! -d "${SYSROOT}/usr" ]]; then
    echo "Sysroot missing at ${SYSROOT}/usr — see prereqs at top of this script." >&2
    exit 1
fi

# pkg-config sysroot rewriting, scoped to the aarch64 target ONLY. Using
# unscoped PKG_CONFIG_SYSROOT_DIR/LIBDIR would also affect the host build
# of build.rs (and build-dependencies like glib-build-tools → glib-sys),
# which would then try to link x86_64 object files against aarch64 .so
# — classic cross-compile gotcha. The `_aarch64_unknown_linux_gnu` suffix
# is pkg-config-rs's convention for per-target configuration.
export PKG_CONFIG_SYSROOT_DIR_aarch64_unknown_linux_gnu="${SYSROOT}"
export PKG_CONFIG_LIBDIR_aarch64_unknown_linux_gnu="${SYSROOT}/usr/lib/aarch64-linux-gnu/pkgconfig:${SYSROOT}/usr/lib/pkgconfig:${SYSROOT}/usr/share/pkgconfig"
export PKG_CONFIG_ALLOW_CROSS_aarch64_unknown_linux_gnu=1

# Match the flatpak manifest's build-time constants so runtime paths
# resolve inside the flatpak sandbox we deploy into.
export APP_ID="io.github.janekbt.Meditate"
export APP_VERSION="26.4.2"
export PKGDATADIR="/app/share/meditate"
export LOCALEDIR="/app/share/locale"

# Cross-toolchain config is scoped to this script via env vars rather than
# a checked-in .cargo/config.toml, because the sysroot path is host-specific
# ($HOME) and the linker doesn't exist inside the aarch64-QEMU flatpak CI
# container — committing it would break `flatpak-builder --arch=aarch64`.
#
# target-cpu=cortex-a53 tunes codegen for the Librem 5's i.MX 8M Quad (4×
# A53); A55/A57/A72/A76 all tolerate A53-tuned code. --sysroot makes the
# linker rewrite absolute paths in GNU ld scripts (e.g. libm.so's
# "GROUP (/usr/lib/aarch64-linux-gnu/libm.so.6 ...)") through the flatpak
# SDK sysroot instead of the x86 host's nonexistent aarch64 libm.
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="aarch64-linux-gnu-gcc"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C target-cpu=cortex-a53 -C link-arg=--sysroot=${SYSROOT} -C link-arg=-Wl,-rpath-link=${SYSROOT}/usr/lib/aarch64-linux-gnu -C link-arg=-Wl,-rpath-link=${SYSROOT}/usr/lib"

cd "$(dirname "$0")/.."
cargo build --release --target aarch64-unknown-linux-gnu "$@"

echo
echo "Binary: $(pwd)/target/aarch64-unknown-linux-gnu/release/meditate"
ls -lh target/aarch64-unknown-linux-gnu/release/meditate
file target/aarch64-unknown-linux-gnu/release/meditate
