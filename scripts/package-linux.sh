#!/usr/bin/env bash
# Build the AnotherOne Linux AppImage from the Flutter desktop +
# the mcp-shim cargo binary.
#
# Output: target/release/linux/AnotherOne-<arch>.AppImage
#
# Layout assembled under target/release/linux/AppDir/:
#   usr/bin/another-one              # Flutter app launcher (wraps the bundle)
#   usr/bin/another-one-mcp-shim     # mcp-shim cargo binary
#   usr/lib/another-one/bundle/...   # Flutter build output (libapp.so + flutter_assets/)
#   usr/share/applications/another-one.desktop
#   usr/share/icons/hicolor/256x256/apps/another-one.png
#
# linuxdeploy synthesizes the root AppRun, top-level .desktop
# symlink, and bundles the binary's dynamic dependencies. It's
# downloaded into target/release/linux/tools/ on first run and
# reused thereafter.

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

FLUTTER_DIR="$ROOT_DIR/$PACKAGE_NAME"
SHIM_RELEASE_DIR="$ROOT_DIR/target/release"
SHIM_BINARY_PATH="$SHIM_RELEASE_DIR/$SHIM_NAME"

PACKAGE_DIR="$ROOT_DIR/target/release/linux"
APPDIR="$PACKAGE_DIR/AppDir"
TOOLS_DIR="$PACKAGE_DIR/tools"
APPIMAGE_OUT="$PACKAGE_DIR/${APP_NAME}-${ARCH}.AppImage"

DESKTOP_SOURCE="$FLUTTER_DIR/packaging/linux/another-one.desktop"
ICON_SOURCE="$FLUTTER_DIR/packaging/linux/another-one.png"

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

echo "==> building $PACKAGE_NAME (flutter desktop, release)"
(
  cd "$FLUTTER_DIR"
  flutter build linux --release
)

FLUTTER_BUNDLE_DIR="$FLUTTER_DIR/build/linux/x64/release/bundle"
if [[ ! -d "$FLUTTER_BUNDLE_DIR" ]]; then
  echo "Expected Flutter bundle at $FLUTTER_BUNDLE_DIR after build." >&2
  exit 1
fi
FLUTTER_BINARY="$FLUTTER_BUNDLE_DIR/$PACKAGE_NAME"
if [[ ! -x "$FLUTTER_BINARY" ]]; then
  echo "Expected Flutter binary at $FLUTTER_BINARY." >&2
  exit 1
fi

echo "==> building $SHIM_NAME (cargo, release)"
(
  cd "$ROOT_DIR"
  cargo build -p "$SHIM_NAME" --release
)

if [[ ! -x "$SHIM_BINARY_PATH" ]]; then
  echo "Expected shim binary at $SHIM_BINARY_PATH after build." >&2
  exit 1
fi

echo "==> assembling AppDir at $APPDIR"
rm -rf "$APPDIR"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/lib/$PACKAGE_NAME" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# Copy the entire Flutter bundle (binary + libapp.so +
# data/icudtl.dat + data/flutter_assets/...) under
# /usr/lib/another-one/bundle, then drop a tiny launcher under
# /usr/bin that exec()s into it. Keeping the bundle layout
# intact preserves Flutter's relative-path lookups
# (`data/flutter_assets/`, `lib/libapp.so`, `lib/libflutter_linux_gtk.so`).
BUNDLE_DEST="$APPDIR/usr/lib/$PACKAGE_NAME/bundle"
cp -r "$FLUTTER_BUNDLE_DIR" "$BUNDLE_DEST"

cat > "$APPDIR/usr/bin/$PACKAGE_NAME" <<'LAUNCHER'
#!/bin/sh
# AppImage launcher — exec into the Flutter bundle's binary
# while preserving the bundle's relative-path lookups.
APPDIR_FALLBACK="$(dirname "$(readlink -f "$0")")/../.."
APPDIR_BASE="${APPDIR:-$APPDIR_FALLBACK}"
exec "$APPDIR_BASE/usr/lib/another-one/bundle/another-one" "$@"
LAUNCHER
chmod 0755 "$APPDIR/usr/bin/$PACKAGE_NAME"

install -m 0755 "$SHIM_BINARY_PATH" "$APPDIR/usr/bin/$SHIM_NAME"

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
echo "==> running linuxdeploy"
(
  cd "$PACKAGE_DIR"
  NO_STRIP=1 \
  OUTPUT="$(basename "$APPIMAGE_OUT")" \
  "$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --executable "$BUNDLE_DEST/$PACKAGE_NAME" \
    --executable "$APPDIR/usr/bin/$SHIM_NAME" \
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
