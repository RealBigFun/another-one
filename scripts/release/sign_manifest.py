#!/usr/bin/env python3
"""Sign `latest.json` with the publisher's Ed25519 private key.

The desktop app verifies this signature before trusting any asset
URLs or checksums. The signed bytes are the *exact* manifest bytes
written by `build_manifest.py` — no re-formatting or
canonicalization. CI loads the private key from the
`ANOTHER_ONE_UPDATE_SIGN_PRIVKEY_HEX` secret as a hex string.
"""

from __future__ import annotations

import argparse
import os
import sys

from nacl.signing import SigningKey


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--signature", required=True)
    args = parser.parse_args()

    privkey_hex = os.environ.get("ED25519_PRIVKEY_HEX", "").strip()
    if not privkey_hex:
        print("ED25519_PRIVKEY_HEX is empty", file=sys.stderr)
        return 1

    try:
        seed = bytes.fromhex(privkey_hex)
    except ValueError as err:
        print(f"invalid hex: {err}", file=sys.stderr)
        return 1

    if len(seed) != 32:
        print(f"private key seed must be 32 bytes, got {len(seed)}", file=sys.stderr)
        return 1

    with open(args.manifest, "rb") as fh:
        manifest_bytes = fh.read()

    signature = SigningKey(seed).sign(manifest_bytes).signature
    # Hex on disk so curl/sha256sum-friendly tooling stays simple
    # — the desktop verifier accepts either raw 64-byte or hex
    # signatures.
    with open(args.signature, "w", encoding="utf-8") as fh:
        fh.write(signature.hex())
        fh.write("\n")
    print(f"wrote {args.signature}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
