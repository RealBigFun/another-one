#!/usr/bin/env bash
# Build the release binaries inside the pinned Ubuntu 22.04 + Rust
# container so the output links against GLIBC 2.35 and ships portable
# to any mainstream modern distro. The linuxdeploy step in
# scripts/package-linux.sh still runs on the host — that's fine, it
# only reads the binary and excludes display-layer libs, neither of
# which depends on the host GLIBC.
#
# Outputs:
#   target/docker-linux/release/another-one
#   target/docker-linux/release/another-one-mcp-shim
#
# Container dep caches survive across runs via the named volumes
# `another-one-cargo-registry` and `another-one-cargo-git`, so only
# the first build pays the crates.io-download cost.

set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
IMAGE_TAG="another-one-linux-builder:jammy-rust1.92"
CONTAINER_TARGET_DIR_REL="target/docker-linux"
CONTAINER_TARGET_DIR="$ROOT_DIR/$CONTAINER_TARGET_DIR_REL"

# Host cache dir for crates.io registry + git checkouts, owned by the
# host user so the container can read/write as `--user $(id -u):$(id -g)`
# without fighting Docker's default root-owned named volumes.
CACHE_DIR="${ANOTHER_ONE_DOCKER_CACHE_DIR:-$HOME/.cache/another-one-docker}"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required for the containerized Linux build." >&2
  echo "install docker, or run scripts/package-linux.sh with ALLOW_HOST_BUILD=1" >&2
  echo "to fall back to the host toolchain (non-portable binary)." >&2
  exit 1
fi

# --network=host for both build and run: on Fedora + firewalld the
# default Docker bridge has no outbound path, so apt-get and
# crates.io fetches time out. Using the host network namespace
# sidesteps the iptables/firewalld interaction entirely. Safe here
# because we trust the image contents and there's nothing sensitive
# listening on the host to be exposed.
echo "==> ensuring builder image $IMAGE_TAG"
docker build \
  --network=host \
  --tag "$IMAGE_TAG" \
  --file "$ROOT_DIR/scripts/linux/Dockerfile" \
  "$ROOT_DIR/scripts/linux"

# Host-bind-mount the cargo cache dirs. Created with host-user
# ownership so `--user $(id -u):$(id -g)` in the container can
# read/write them without the root-owned-volume permission friction
# Docker's named volumes default to.
mkdir -p "$CACHE_DIR/cargo-registry" "$CACHE_DIR/cargo-git"
mkdir -p "$CONTAINER_TARGET_DIR"

echo "==> building release binaries in container"
docker run --rm \
  --network=host \
  --user "$(id -u):$(id -g)" \
  -v "$ROOT_DIR":/src \
  -v "$CACHE_DIR/cargo-registry":/opt/cargo/registry \
  -v "$CACHE_DIR/cargo-git":/opt/cargo/git \
  -e CARGO_TARGET_DIR="/src/$CONTAINER_TARGET_DIR_REL" \
  -e CARGO_HOME=/opt/cargo \
  -e ANOTHER_ONE_BUILD_FULL_SHA \
  -e ANOTHER_ONE_UPDATE_MANIFEST_URL \
  -e ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX \
  -w /src \
  "$IMAGE_TAG" \
  cargo build -p another-one -p another-one-mcp-shim --release

echo "==> container build complete"
ls -lh "$CONTAINER_TARGET_DIR/release/another-one" \
       "$CONTAINER_TARGET_DIR/release/another-one-mcp-shim"
