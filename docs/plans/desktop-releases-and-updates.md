
# Desktop releases and in-app updates

> Plan: every merge to `main` publishes a publicly downloadable desktop release, and release builds discover, download, and stage newer builds automatically while exposing manual controls in Settings → General.

#plan #desktop #release #updater

## Goals

- On every push/merge to `main`, create a desktop release for macOS and Linux.
- Make release artifacts and update metadata downloadable without authentication.
- Give each release a stable identity based on the same git commit SHA shown in the titlebar build chip.
- In the desktop app, poll for updates every 10 minutes.
- Add Settings → General with:
  - current build identity,
  - last update-check result,
  - **Check for updates** button,
  - **Install update** button disabled until an update has been downloaded and verified.
- If a newer release exists, download it in the background and enable install when the payload is ready.

## Non-goals for the first pass

- Delta updates. Download the full DMG/AppImage/update payload.
- Windows support. This app targets macOS and Linux only.
- Silent forced restarts. The app may download automatically, but install/relaunch should be user initiated.
- Auto-updating debug/dev builds by default. Manual check may be useful for testing, but automatic polling should be release-build-only unless explicitly overridden.

## Release identity decision

Use the full git commit SHA as the primary desktop release/build version. The titlebar can keep showing the short chip, e.g. `eca0eb0`, but release/update machinery should use the full SHA.

- `commit_sha`: full git commit SHA from `github.sha`; this is the canonical identity used for release tags, manifest identity, verification, and equality checks.
- `short_sha`: first 7–12 characters shown in the titlebar/settings UI and release names only.
- `release_id`: full commit SHA.
- GitHub release tag in the public release repo: `desktop-{commit_sha}`.
- GitHub release display name: `AnotherOne Desktop {short_sha}`.
- `cargo_version`: `desktop/Cargo.toml` package version, e.g. `0.1.0`, kept as secondary metadata for macOS bundle fields and future user-facing product milestones. It should not drive update detection in the first pass.

Why SHA-first? A release is created for every merge to `main`, and multiple merges may happen without bumping `desktop/Cargo.toml`. The commit SHA is already what the app displays today, is unique to the exact source snapshot, and lines up with CI/release provenance.

Important implementation note: `desktop/build.rs` currently emits a short SHA. For update correctness, add a full-SHA build env value too, while continuing to display the short SHA in the chip. Store the full SHA for correctness; show the short SHA for readability.

## Public release hosting

Decision: keep release/update artifacts in a **separate public repository**, e.g. `another-one-releases`. The source repository can stay private; the app never needs GitHub credentials because it only talks to public release URLs.

The private source repo’s CI will build desktop artifacts on `main`, then publish them into the public release repo using a scoped token or deploy key stored as a GitHub Actions secret.

The app should only depend on public, static URLs in the release repo, for example:

- `https://github.com/<org>/another-one-releases/releases/latest/download/latest.json`
- asset URLs referenced inside `latest.json`

The release workflow should upload immutable artifacts plus `latest.json` to the public repo. GitHub’s “latest release” URL gives the app a stable endpoint while each underlying release remains uniquely tagged by SHA.

## Release artifacts

For each release, produce:

- `AnotherOne-macos-{arch}-{release_id}.dmg` for manual public download.
- `AnotherOne-macos-{arch}-{release_id}.app.tar.gz` or `.zip` for updater install payload.
- `AnotherOne-linux-{arch}-{release_id}.AppImage` for manual download and updater payload.
- `SHA256SUMS` covering every asset.
- `latest.json` update manifest.
- `latest.json.sig` Ed25519 signature over the exact manifest bytes.

Initial supported matrix:

- macOS `aarch64-apple-darwin` and `x86_64-apple-darwin` if CI time allows; otherwise start with the runner-native arch and make unsupported arch explicit in docs/UI.
- Linux `x86_64` AppImage using the existing Ubuntu 22.04 container path in `scripts/package-linux.sh`.

## Manifest schema

`latest.json` should be intentionally small and versioned:

```json
{
  "schema_version": 1,
  "app": "another-one-desktop",
  "channel": "stable",
  "release_id": "eca0eb0b2f8a9d0e111122223333444455556666",
  "short_sha": "eca0eb0",
  "commit_sha": "eca0eb0b2f8a9d0e111122223333444455556666",
  "cargo_version": "0.1.0",
  "build_number": 42,
  "published_at": "2026-04-26T12:34:56Z",
  "release_notes_url": "https://github.com/<org>/another-one-releases/releases/tag/desktop-eca0eb0b2f8a9d0e111122223333444455556666",
  "assets": [
    {
      "os": "macos",
      "arch": "aarch64",
      "kind": "app-tar-gz",
      "url": "https://github.com/<org>/another-one-releases/releases/download/desktop-eca0eb0b2f8a9d0e111122223333444455556666/AnotherOne-macos-aarch64-eca0eb0b2f8a9d0e111122223333444455556666.app.tar.gz",
      "sha256": "...",
      "size_bytes": 123456789
    },
    {
      "os": "linux",
      "arch": "x86_64",
      "kind": "appimage",
      "url": "https://github.com/<org>/another-one-releases/releases/download/desktop-eca0eb0b2f8a9d0e111122223333444455556666/AnotherOne-linux-x86_64-eca0eb0b2f8a9d0e111122223333444455556666.AppImage",
      "sha256": "...",
      "size_bytes": 123456789
    }
  ]
}
```

Comparison rules:

1. If remote `commit_sha == local commit_sha`, the app is up to date.
2. If remote `commit_sha != local commit_sha`, update exists when remote `build_number`/`published_at` is newer than the local build metadata.
3. Do not try to order raw SHAs; they are identifiers, not sortable versions. Use `build_number`, `published_at`, or the fact that `latest.json` points at the current stable release.
4. `cargo_version` is display/packaging metadata only in the first pass.
5. If the manifest lacks an asset matching the current OS/arch, show an informational/manual-download state, not an error loop.

## CI plan

Add `.github/workflows/desktop-release.yml`:

- Trigger: `push` to `main`.
- Permissions: read/build permissions in the source repo, plus a scoped token or SSH deploy key that can create releases in the public `another-one-releases` repo.
- Compute:
  - `CARGO_VERSION` from `cargo metadata` or `desktop/Cargo.toml`,
  - `COMMIT_SHA=${GITHUB_SHA}`,
  - `SHORT_SHA=${GITHUB_SHA::7}`,
  - `RELEASE_ID=${COMMIT_SHA}`,
  - `TAG=desktop-${COMMIT_SHA}`,
  - monotonic `BUILD_NUMBER=${GITHUB_RUN_NUMBER}`.
- Build jobs:
  - macOS: run/extend `scripts/package-macos.sh` with version/env overrides.
  - Linux: run `scripts/package-linux.sh` using the existing containerized build.
- Package script updates:
  - accept `CARGO_VERSION`, `RELEASE_ID`, and output directory/name overrides,
  - set macOS `CFBundleShortVersionString` to `CARGO_VERSION`,
  - set macOS `CFBundleVersion` to a monotonic build number or short SHA-compatible build string,
  - include arch and release id in artifact names.
- Release job:
  - download build artifacts,
  - generate `SHA256SUMS`,
  - generate `latest.json`,
  - sign `latest.json` with an Ed25519 private key stored as a GitHub Actions secret,
  - use `gh release create --repo <org>/another-one-releases` or the GitHub API to create GitHub Release `TAG`, named `AnotherOne Desktop {SHORT_SHA}`,
  - upload all assets to the public release repo,
  - mark as non-prerelease for `main`.

macOS distribution hardening:

- `scripts/package-macos.sh` supports Developer ID signing and notarization
  through `MACOS_SIGN_IDENTITY`, `MACOS_NOTARIZE=1`, and notarytool credentials.
- Local builds still fall back to ad-hoc signing when those values are absent.
- Keep updater payload signatures separate from platform signing; the app should verify the manifest signature and SHA256 before enabling **Install update**.

## App architecture plan

Add a small desktop updater module, e.g. `desktop/src/updater.rs`, rather than adding this logic directly to `app.rs`.

Suggested types:

- `BuildIdentity`: local `cargo_version`, full `commit_sha`, `short_sha`, `build_time`, `dev/release`.
- `UpdateManifest`: deserialized `latest.json`.
- `UpdateAsset`: OS/arch-specific downloadable asset.
- `UpdateState`:
  - `Idle`,
  - `Checking`,
  - `UpToDate`,
  - `UpdateFound`,
  - `Downloading { progress }`,
  - `ReadyToInstall { path }`,
  - `Installing`,
  - `Error { message }`.
- `UpdaterCommand`: `CheckNow`, `Download`, `Install`.
- `UpdaterEvent`: check/download/install results sent back to the GPUI app.

Implementation notes:

- Use a background worker/thread or GPUI background task so network and file IO never run on the render path.
- Add dependencies with Rustls/no-OpenSSL defaults for macOS/Linux portability, e.g. HTTP client, `sha2`, and Ed25519 verification. A SemVer parser is optional later if `cargo_version` becomes more than display metadata.
- Store downloads under a per-user cache directory such as `~/.cache/another-one/updates/` on Linux and the platform cache directory on macOS via `dirs`.
- Download to `*.part`, fsync/flush, verify SHA256, then atomically rename to the final cached filename.
- Verify `latest.json.sig` before trusting asset URLs or checksums.
- Coalesce checks: if a check/download is already running, the manual button should reuse that state instead of starting a second request.
- All user-facing errors and notifications must go through the existing toast helpers.

## Polling behavior

- On app startup in release builds, schedule the first check after a short delay, e.g. 30–60 seconds, to avoid slowing launch.
- Then check every 10 minutes while the app is running.
- Manual **Check for updates** ignores the interval but still respects “already checking/downloading”.
- If an update exists, start the background download automatically.
- If download succeeds, persist the `ReadyToInstall` state and enable **Install update**.
- If download fails, show a toast once and keep the manual button available for retry.

## Settings → General UI plan

Add `SettingsSection::General` as the first item in `desktop/src/settings_page.rs`.

Content:

- Header: “General”.
- “Build” row showing the short commit SHA from `desktop/src/build_info.rs`, the full SHA in copy/tooltip details, `CARGO_PKG_VERSION` as secondary metadata, and release/debug marker.
- “Updates” row with:
  - status text, e.g. “Up to date”, “Checking…”, “Downloading eca0eb0…”, “Update ready to install”,
  - **Check for updates** button,
  - **Install update** button.
- Disable **Check for updates** while checking/downloading.
- Disable **Install update** unless state is `ReadyToInstall`.
- Use toasts for:
  - check failed,
  - download failed,
  - update downloaded and ready,
  - install could not start.

## Install behavior

### Linux AppImage

- Prefer replacing the running AppImage path from the `APPIMAGE` environment variable.
- On **Install update**, spawn a small helper process/script outside the AppImage that:
  1. waits for the current process to exit,
  2. unlinks/replaces the old AppImage with the downloaded one,
  3. sets executable bit,
  4. relaunches the new AppImage.
- If `APPIMAGE` is missing, do not guess. Open the downloaded file or containing folder and show a toast explaining manual installation.

### macOS

- Public manual download remains a DMG.
- For in-app install, prefer an updater-specific `.app.tar.gz`/`.zip` asset so the helper can replace the existing `.app` bundle directly.
- On **Install update**, spawn a helper outside the app bundle that:
  1. resolves the current `.app` bundle path,
  2. waits for the app to quit,
  3. replaces the bundle with the verified downloaded bundle,
  4. relaunches the app.
- If the app is not running from a normal `.app` bundle, open the downloaded DMG/payload and show a manual-install toast.
- Before broad distribution, use Developer ID signing and notarization so replacement and launch do not create Gatekeeper friction.

## Implementation phases

1. **Release pipeline MVP**
   - Add GitHub Actions release workflow.
   - Teach package scripts to accept cargo-version/release-id/output overrides.
   - Publish macOS/Linux artifacts, checksums, and manifest to a public release surface.

2. **Updater core**
   - Add local build identity helpers using existing `build_info.rs` plus package version, including a full commit SHA value.
   - Fetch and verify manifest/signature.
   - Select matching asset by OS/arch.
   - Compare local vs remote commit/build identity.
   - Download and verify payload in the background.

3. **Settings → General**
   - Add General settings section.
   - Render current build SHA and secondary package version.
   - Wire **Check for updates** and disabled/enabled **Install update** button states.
   - Surface all user-facing messages via toasts.

4. **Install helpers**
   - Linux AppImage replacement/relaunch helper.
   - macOS `.app` replacement/relaunch helper or manual-open fallback.

5. **Hardening**
   - Configure production macOS signing/notarization secrets in CI.
   - Manifest signing key rotation docs.
   - Resume/cleanup partial downloads.
   - Tests for manifest parsing, update comparison, OS/arch asset selection, checksum verification, and state transitions.

## Acceptance criteria

- Merging to `main` creates a GitHub Release in the public `another-one-releases` repo with macOS and Linux desktop artifacts.
- A browser/incognito `curl -I` to `latest.json` and release assets succeeds without authentication.
- Release tags and manifest identity use the full commit SHA; release names and UI use the titlebar-style short SHA.
- A release build of the app checks for updates automatically every 10 minutes.
- Settings → General includes **Check for updates**.
- If a newer manifest is published, the app downloads the matching asset in the background.
- The **Install update** button is disabled before download verification and enabled after verification.
- Failed checks/downloads show app toasts and do not block normal app usage.
- Linux AppImage install path works when launched from AppImage; otherwise the app provides a clear manual-install fallback.
- macOS has a working install path or a deliberate manual-install fallback until signing/notarization and bundle replacement are complete.
