#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

scripts=(
  scripts/slint/common.sh
  scripts/slint/linux-dev.sh
  scripts/slint/linux-release.sh
  scripts/slint/macos-build.sh
  scripts/slint/android-apk.sh
  scripts/slint/ios-simulator-build.sh
  scripts/slint/verify-platform-scripts.sh
)

echo "==> checking Slint platform script syntax"
for script in "${scripts[@]}"; do
  bash -n "$ROOT_DIR/$script"
  echo "ok: $script"
done

echo "==> checking Slint platform script help paths"
for script in \
  scripts/slint/linux-dev.sh \
  scripts/slint/linux-release.sh \
  scripts/slint/macos-build.sh \
  scripts/slint/android-apk.sh \
  scripts/slint/ios-simulator-build.sh
do
  "$ROOT_DIR/$script" --help >/dev/null
  echo "ok: $script --help"
done
