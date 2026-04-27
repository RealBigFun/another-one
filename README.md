# AnotherOne

AnotherOne is a greenfield desktop and mobile app built around local agent
workflows.

## Development

Run the desktop app:

```sh
cargo run -p desktop
```

The desktop target is macOS and Linux.

## Releasing for your own Mac

On macOS, build a locally signed `.app` bundle and `.dmg` with:

```sh
scripts/package-macos.sh
```

The package lands under `target/release/macos/`. To open the generated DMG
when packaging finishes, pass `--open`:

```sh
scripts/package-macos.sh --open
```

This is intended for personal installs on your own Mac. It uses ad-hoc
codesigning, so it is not a notarized public distribution build.

## Public macOS releases

Downloaded DMGs must be signed with a Developer ID Application certificate and
notarized by Apple. Otherwise Gatekeeper shows an "Apple could not verify"
malware warning after download.

For a distributable macOS build, provide:

```sh
MACOS_SIGN_IDENTITY="Developer ID Application: Example, Inc. (TEAMID)" \
MACOS_NOTARIZE=1 \
APPLE_ID="apple-id@example.com" \
APPLE_TEAM_ID="TEAMID" \
APPLE_APP_SPECIFIC_PASSWORD="xxxx-xxxx-xxxx-xxxx" \
scripts/package-macos.sh
```

CI can import the Developer ID certificate from
`ANOTHER_ONE_MACOS_CERTIFICATE_P12_BASE64`. For notarized release builds, set
these GitHub Actions secrets:

- `ANOTHER_ONE_MACOS_CERTIFICATE_P12_BASE64`
- `ANOTHER_ONE_MACOS_CERTIFICATE_PASSWORD`
- `ANOTHER_ONE_MACOS_KEYCHAIN_PASSWORD`
- `ANOTHER_ONE_MACOS_SIGN_IDENTITY`
- `ANOTHER_ONE_MACOS_NOTARIZE` set to `1`
- `ANOTHER_ONE_APPLE_ID`
- `ANOTHER_ONE_APPLE_TEAM_ID`
- `ANOTHER_ONE_APP_SPECIFIC_PASSWORD`
