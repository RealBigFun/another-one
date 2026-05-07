#!/usr/bin/env bash
# Build the AnotherOne desktop AppImage.
#
# Output: target/release/linux/AnotherOne-x86_64.AppImage
#
# Layout assembled under target/release/linux/AppDir/:
#   usr/bin/another-one
#   usr/bin/another-one-mcp-shim
#   usr/share/another-one/assets/...        (loaded via $APPDIR by assets.rs)
#   usr/share/applications/another-one.desktop
#   usr/share/icons/hicolor/256x256/apps/another-one.png
#
# linuxdeploy synthesizes the root AppRun, top-level .desktop symlink,
# and bundles the binary's dynamic dependencies. It's downloaded into
# target/release/linux/tools/ on first run and reused thereafter.

set -euo pipefail

OPEN_AFTER=0
INSTALL_AFTER=0

usage() {
  cat <<EOF
Usage: $0 [--open] [--install]

Build the AnotherOne AppImage from the current source tree.

Options:
  --open       Launch the AppImage after a successful build.
  --install    Replace the installed AppImage at \$INSTALL_PATH after
               a successful build (default: \$HOME/Applications/AnotherOne.AppImage).
               Designed for wiring into an in-app "Action" so a click
               does build + install in one shot. Works even while the
               currently-installed binary is running: the file is
               unlinked (running process keeps its inode) before the
               new one is written.
  -h, --help   Show this help message.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --open)
      OPEN_AFTER=1
      shift
      ;;
    --install)
      INSTALL_AFTER=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

INSTALL_PATH="${INSTALL_PATH:-$HOME/Applications/AnotherOne.AppImage}"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Linux packaging requires Linux; current platform is $(uname -s)." >&2
  exit 1
fi

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

APP_NAME="AnotherOne"
PACKAGE_NAME="another-one"
SHIM_NAME="another-one-mcp-shim"
ARCH="$(uname -m)"

# By default binaries come from the pinned Ubuntu 22.04 container
# (GLIBC 2.35 floor, reproducible). Set ALLOW_HOST_BUILD=1 to skip
# Docker and use the host toolchain — faster iteration but the
# AppImage will only run on distros with ≥ host GLIBC.
ALLOW_HOST_BUILD="${ALLOW_HOST_BUILD:-0}"
if [[ "$ALLOW_HOST_BUILD" -eq 1 ]]; then
  RELEASE_DIR="$ROOT_DIR/target/release"
else
  RELEASE_DIR="$ROOT_DIR/target/docker-linux/release"
fi

PACKAGE_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/release/linux}"
APPDIR="$PACKAGE_DIR/AppDir"
TOOLS_DIR="$PACKAGE_DIR/tools"
APPIMAGE_OUT="$PACKAGE_DIR/${APP_NAME}-${ARCH}.AppImage"

RELEASE_ID="${RELEASE_ID:-}"
ARTIFACT_PREFIX="${ARTIFACT_PREFIX:-AnotherOne}"

if [[ -n "$RELEASE_ID" ]]; then
  TRUST_PUBKEY_HEX="${ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX:-}"
  if [[ -z "$TRUST_PUBKEY_HEX" ]]; then
    echo "ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX is required when RELEASE_ID is set." >&2
    echo "Without it, the packaged AppImage cannot verify update manifests." >&2
    exit 1
  fi
  if [[ ! "$TRUST_PUBKEY_HEX" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo "ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX must be a 32-byte Ed25519 public key encoded as 64 hex characters." >&2
    exit 1
  fi
fi

BINARY_PATH="$RELEASE_DIR/$PACKAGE_NAME"
SHIM_BINARY_PATH="$RELEASE_DIR/$SHIM_NAME"

ASSETS_SOURCE="$ROOT_DIR/app/assets"
DESKTOP_SOURCE="$ROOT_DIR/app/assets/app-icon/linux/another-one.desktop"
ICON_SOURCE="$ROOT_DIR/app/assets/app-icon/linux/another-one.png"

LINUXDEPLOY_VERSION="continuous"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/${LINUXDEPLOY_VERSION}/linuxdeploy-${ARCH}.AppImage"
LINUXDEPLOY="$TOOLS_DIR/linuxdeploy-${ARCH}.AppImage"

if [[ ! -f "$DESKTOP_SOURCE" ]]; then
  echo "Expected Linux .desktop template at $DESKTOP_SOURCE." >&2
  exit 1
fi
if [[ ! -f "$ICON_SOURCE" ]]; then
  echo "Expected Linux app icon at $ICON_SOURCE." >&2
  exit 1
fi

if [[ "$ALLOW_HOST_BUILD" -eq 1 ]]; then
  echo "==> building $PACKAGE_NAME + $SHIM_NAME (release, host toolchain)"
  (
    cd "$ROOT_DIR"
    cargo build -p "$PACKAGE_NAME" -p "$SHIM_NAME" --release
  )
else
  "$ROOT_DIR/scripts/linux/build-in-container.sh"
fi

if [[ ! -x "$BINARY_PATH" ]]; then
  echo "Expected release binary at $BINARY_PATH after build." >&2
  exit 1
fi
if [[ ! -x "$SHIM_BINARY_PATH" ]]; then
  echo "Expected shim binary at $SHIM_BINARY_PATH after build." >&2
  exit 1
fi

echo "==> assembling AppDir at $APPDIR"
rm -rf "$APPDIR"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/$PACKAGE_NAME" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps"

install -m 0755 "$BINARY_PATH" "$APPDIR/usr/bin/$PACKAGE_NAME"
install -m 0755 "$SHIM_BINARY_PATH" "$APPDIR/usr/bin/$SHIM_NAME"

# Copy the entire app/assets/ tree as the runtime asset root.
# `app/src/assets.rs::linux_appimage_resource_root()` resolves
# this via $APPDIR, so the binary loads fonts, icons, and SVGs from
# inside the AppImage instead of the build-time CARGO_MANIFEST_DIR.
cp -r "$ASSETS_SOURCE" "$APPDIR/usr/share/$PACKAGE_NAME/"

install -m 0644 "$DESKTOP_SOURCE" "$APPDIR/usr/share/applications/$PACKAGE_NAME.desktop"
install -m 0644 "$ICON_SOURCE" "$APPDIR/usr/share/icons/hicolor/256x256/apps/$PACKAGE_NAME.png"

echo "==> ensuring linuxdeploy at $LINUXDEPLOY"
mkdir -p "$TOOLS_DIR"
if [[ ! -x "$LINUXDEPLOY" ]]; then
  echo "    downloading $LINUXDEPLOY_URL"
  curl --location --fail --silent --show-error \
    --output "$LINUXDEPLOY" "$LINUXDEPLOY_URL"
  chmod +x "$LINUXDEPLOY"
fi

# linuxdeploy writes the AppImage into the current directory using
# $OUTPUT as its filename, so cd into the package dir first.
#
# NO_STRIP=1: linuxdeploy's bundled `strip` (binutils ~2.38) chokes
# on `.relr.dyn` sections in modern Fedora's libraries (built with
# `-Wl,-z,pack-relative-relocs`). Skipping strip costs ~a few MiB of
# AppImage size and adds nothing to runtime cost. Drop this when
# upstream linuxdeploy bundles a newer binutils.
#
# --exclude-library: display-layer libs must come from the host
# system, not the AppImage, or you get ABI mismatches between the
# bundled libxkbcommon and the running compositor (segfault on
# startup was how this bit us). linuxdeploy's built-in blacklist
# catches `/lib64/libxcb.so.1` but misses the user-local
# `/usr/local/lib64/libxkbcommon*`, hence the explicit list.
#
# PATCHELF override: linuxdeploy bundles patchelf 0.15.0, which
# corrupts the .text section of large Rust binaries (the produced
# binary has ~37MB of 0x58/'X' bytes overwriting code, segfaulting
# at _start). Force linuxdeploy to use the host patchelf if it's
# new enough; bail out if not, since silently producing a broken
# AppImage is worse than failing the build.
HOST_PATCHELF="$(command -v patchelf || true)"
if [[ -z "$HOST_PATCHELF" ]]; then
  echo "patchelf not found on PATH. Install patchelf >= 0.16 (e.g. 'sudo dnf install patchelf')." >&2
  exit 1
fi
HOST_PATCHELF_VERSION="$("$HOST_PATCHELF" --version | awk '{print $2}')"
HOST_PATCHELF_MAJOR="${HOST_PATCHELF_VERSION%%.*}"
HOST_PATCHELF_MINOR_RAW="${HOST_PATCHELF_VERSION#*.}"
HOST_PATCHELF_MINOR="${HOST_PATCHELF_MINOR_RAW%%.*}"
if (( HOST_PATCHELF_MAJOR == 0 && HOST_PATCHELF_MINOR < 16 )); then
  echo "patchelf $HOST_PATCHELF_VERSION at $HOST_PATCHELF is too old; need >= 0.16." >&2
  echo "linuxdeploy's bundled patchelf 0.15 corrupts large binaries." >&2
  exit 1
fi
echo "==> running linuxdeploy (patchelf=$HOST_PATCHELF v$HOST_PATCHELF_VERSION)"
(
  cd "$PACKAGE_DIR"
  NO_STRIP=1 \
  PATCHELF="$HOST_PATCHELF" \
  OUTPUT="$(basename "$APPIMAGE_OUT")" \
  "$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --desktop-file "$APPDIR/usr/share/applications/$PACKAGE_NAME.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/$PACKAGE_NAME.png" \
    --exclude-library 'libxkbcommon.so*' \
    --exclude-library 'libxkbcommon-x11.so*' \
    --exclude-library 'libxcb-xkb.so*' \
    --exclude-library 'libXau.so*' \
    --output appimage
)

if [[ ! -f "$APPIMAGE_OUT" ]]; then
  echo "linuxdeploy did not produce $APPIMAGE_OUT." >&2
  exit 1
fi

echo ""
echo "AppImage built: $APPIMAGE_OUT"
ls -lh "$APPIMAGE_OUT"

if [[ -n "$RELEASE_ID" ]]; then
  case "$ARCH" in
    aarch64|x86_64) ARCH_LABEL="$ARCH" ;;
    arm64) ARCH_LABEL="aarch64" ;;
    *) ARCH_LABEL="$ARCH" ;;
  esac
  RELEASE_APPIMAGE="$PACKAGE_DIR/${ARTIFACT_PREFIX}-linux-${ARCH_LABEL}.AppImage"
  cp -f "$APPIMAGE_OUT" "$RELEASE_APPIMAGE"
  chmod +x "$RELEASE_APPIMAGE"
  echo "Release-named copy: $RELEASE_APPIMAGE"
fi

if [[ "$INSTALL_AFTER" -eq 1 ]]; then
  echo ""
  echo "==> installing to $INSTALL_PATH"
  mkdir -p "$(dirname "$INSTALL_PATH")"
  # Unlink before copy so the install works even when the existing
  # AppImage at INSTALL_PATH is currently running. Linux is happy to
  # unlink a running ELF — the kernel keeps the inode alive for the
  # running process, and a fresh inode is created for the new file.
  # Without this, `cp` would fail with ETXTBSY ("Text file busy").
  if [[ -e "$INSTALL_PATH" ]]; then
    rm -f "$INSTALL_PATH"
  fi
  cp "$APPIMAGE_OUT" "$INSTALL_PATH"
  chmod +x "$INSTALL_PATH"
  echo "installed: $INSTALL_PATH"
  echo "(close and reopen AnotherOne to use the new build)"
fi

if [[ "$OPEN_AFTER" -eq 1 ]]; then
  echo "==> launching"
  exec "$APPIMAGE_OUT"
fi
