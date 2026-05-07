#!/usr/bin/env python3
"""Generate the `latest.json` update manifest for a desktop release.

This script emits the schema consumed by `app/src/updater.rs`. CI runs
it from `.github/workflows/desktop-release.yml` after the per-platform
build jobs have uploaded their artifacts into a single staging directory.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
from dataclasses import dataclass


SCHEMA_VERSION = 1
APP_ID = "another-one-desktop"
CHANNEL = "stable"


@dataclass(frozen=True)
class Asset:
    os: str
    arch: str
    kind: str
    filename: str

    def to_manifest(self, base_url: str, release_dir: str) -> dict:
        path = os.path.join(release_dir, self.filename)
        with open(path, "rb") as fh:
            digest = hashlib.sha256(fh.read()).hexdigest()
        return {
            "os": self.os,
            "arch": self.arch,
            "kind": self.kind,
            "url": f"{base_url}/{self.filename}",
            "sha256": digest,
            "size_bytes": os.path.getsize(path),
        }


# Filename patterns we expect to see in the staging directory. New
# kinds (e.g. `.zip`) get added here once the packaging scripts
# emit them.
PATTERNS = [
    (re.compile(r"^AnotherOne-(macos)-(aarch64|x86_64)\.app\.tar\.gz$"), "app-tar-gz"),
    (re.compile(r"^AnotherOne-(macos)-(aarch64|x86_64)\.dmg$"), "dmg"),
    (re.compile(r"^AnotherOne-(linux)-(aarch64|x86_64)\.AppImage$"), "appimage"),
]


def discover_assets(release_dir: str) -> list[Asset]:
    assets: list[Asset] = []
    for entry in sorted(os.listdir(release_dir)):
        for pattern, kind in PATTERNS:
            match = pattern.match(entry)
            if match:
                assets.append(
                    Asset(
                        os=match.group(1),
                        arch=match.group(2),
                        kind=kind,
                        filename=entry,
                    )
                )
                break
    return assets


def select_updater_assets(assets: list[Asset]) -> list[Asset]:
    """Keep one asset per OS+arch+kind, preferring updater payloads.

    The desktop updater wants `.app.tar.gz` over `.dmg` on macOS so
    the install helper can replace the `.app` bundle directly. We
    still ship the DMG as a downloadable, but we don't reference it
    in `latest.json`.
    """
    keep: dict[tuple[str, str], Asset] = {}
    priority = {"app-tar-gz": 0, "appimage": 0, "dmg": 1}
    for asset in assets:
        key = (asset.os, asset.arch)
        existing = keep.get(key)
        if existing is None or priority[asset.kind] < priority[existing.kind]:
            keep[key] = asset
    return [keep[k] for k in sorted(keep)]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-dir", required=True)
    parser.add_argument("--release-id", required=True)
    parser.add_argument("--short-sha", required=True)
    parser.add_argument("--cargo-version", required=True)
    parser.add_argument("--build-number", required=True, type=int)
    parser.add_argument("--published-at", required=True)
    parser.add_argument("--release-repo", required=True)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    base_url = (
        f"https://github.com/{args.release_repo}/releases/download/{args.release_tag}"
    )

    discovered = discover_assets(args.release_dir)
    if not discovered:
        print(f"no recognizable assets found in {args.release_dir}", file=sys.stderr)
        return 1

    selected = select_updater_assets(discovered)
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "app": APP_ID,
        "channel": CHANNEL,
        "release_id": args.release_id,
        "short_sha": args.short_sha,
        "commit_sha": args.release_id,
        "cargo_version": args.cargo_version,
        "build_number": args.build_number,
        "published_at": args.published_at,
        "release_notes_url": (
            f"https://github.com/{args.release_repo}/releases/tag/{args.release_tag}"
        ),
        "assets": [asset.to_manifest(base_url, args.release_dir) for asset in selected],
    }

    with open(args.output, "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, indent=2, sort_keys=True)
        fh.write("\n")
    print(f"wrote {args.output} with {len(selected)} assets")
    return 0


if __name__ == "__main__":
    sys.exit(main())
