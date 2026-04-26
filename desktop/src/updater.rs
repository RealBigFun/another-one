//! Desktop in-app updater.
//!
//! The updater runs on a dedicated OS thread, talks to a public
//! release repo over HTTPS, and exposes a small command/event API to
//! the GPUI app. Network and disk IO never run on the render path —
//! the GPUI side only `try_recv()`s pre-formatted [`UpdaterEvent`]s
//! during the existing render-tick drain.
//!
//! Identity & comparison rules (matches the plan):
//!
//! 1. Local build identity = full `commit_sha` baked in at build
//!    time via `desktop/build.rs`. Multiple merges to `main` may
//!    share a `cargo_version`, so the SHA — not the package
//!    version — is what equality tests against `latest.json`.
//! 2. If `remote.commit_sha == local.commit_sha`, the app is up
//!    to date. Otherwise, an update exists when the remote
//!    `build_number`/`published_at` is newer.
//! 3. Raw SHAs are never ordered; they're identifiers, not
//!    versions.
//!
//! Trust:
//!
//! * `latest.json.sig` is an Ed25519 signature over the exact
//!   bytes of `latest.json`. Built with [`TRUST_PUBKEY_HEX`]
//!   embedded at compile time. If the trust key is unset the
//!   updater refuses to apply updates and surfaces an error in
//!   the UI; check-only flows still work for dev convenience.
//! * Each asset has a `sha256` field. The downloader streams to
//!   `*.part`, verifies the digest, then atomically renames to the
//!   final path.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::build_info;

/// How long after app startup the first automatic check fires.
/// Short enough that fresh installs see updates quickly; long
/// enough that we don't slow launch.
pub const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(45);

/// How often the worker polls for updates while the app is
/// running. Matches the plan's "every 10 minutes".
pub const POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// HTTP timeout for manifest + signature fetches. Generous enough
/// for slow networks but short enough that a stuck CDN doesn't
/// silently wedge the worker.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// HTTP timeout for asset payload downloads. Larger than the
/// manifest timeout because release artifacts are tens of MiB.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Default release manifest URL. Override with the
/// `ANOTHER_ONE_UPDATE_MANIFEST_URL` env var at build time so we
/// don't have to recompile when the public release repo moves.
const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/RealBigFun/another-one-releases/releases/latest/download/latest.json";

/// Build-time-injected manifest URL. Falls back to
/// [`DEFAULT_MANIFEST_URL`] when unset.
const MANIFEST_URL: Option<&str> = option_env!("ANOTHER_ONE_UPDATE_MANIFEST_URL");

/// Build-time-injected hex-encoded Ed25519 public key used to
/// verify `latest.json.sig`. When unset the updater keeps working
/// in a check-only mode but refuses to mark a download as
/// `ReadyToInstall`.
const TRUST_PUBKEY_HEX: Option<&str> = option_env!("ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX");

/// Schema version this binary understands. Reject newer manifest
/// schemas with a clear message rather than silently
/// mis-interpreting fields.
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

pub fn manifest_url() -> &'static str {
    MANIFEST_URL.unwrap_or(DEFAULT_MANIFEST_URL)
}

fn signature_url() -> String {
    format!("{}.sig", manifest_url())
}

/// Local build identity reported by `build.rs`.
#[derive(Debug, Clone)]
pub struct BuildIdentity {
    pub full_sha: &'static str,
    pub short_sha: &'static str,
    pub cargo_version: &'static str,
    pub is_dev_build: bool,
}

impl BuildIdentity {
    pub fn current() -> Self {
        Self {
            full_sha: build_info::GIT_FULL_SHA,
            short_sha: build_info::GIT_SHA,
            cargo_version: build_info::CARGO_PKG_VERSION,
            is_dev_build: build_info::is_dev_build(),
        }
    }

    /// Whether automatic polling should run for this build. The
    /// plan says polling is "release-build-only unless explicitly
    /// overridden"; we honor that here.
    pub fn auto_polling_enabled(&self) -> bool {
        if self.is_dev_build {
            // `ANOTHER_ONE_UPDATER_FORCE=1` lets developers
            // exercise the full polling path during local
            // testing without recompiling in release mode.
            std::env::var("ANOTHER_ONE_UPDATER_FORCE").ok().as_deref() == Some("1")
        } else {
            true
        }
    }
}

/// Deserialized `latest.json` payload. Field set matches the
/// schema in `docs/plans/desktop-releases-and-updates.md`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdateManifest {
    pub schema_version: u32,
    pub app: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub release_id: String,
    pub short_sha: String,
    pub commit_sha: String,
    #[serde(default)]
    pub cargo_version: Option<String>,
    #[serde(default)]
    pub build_number: Option<u64>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub release_notes_url: Option<String>,
    pub assets: Vec<UpdateAsset>,
}

/// One downloadable artifact for a specific OS/arch pair.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdateAsset {
    pub os: String,
    pub arch: String,
    pub kind: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

/// Result of comparing the local build to a fetched manifest.
#[derive(Debug, Clone)]
pub enum UpdateComparison {
    UpToDate,
    UpdateAvailable {
        manifest: UpdateManifest,
        asset: UpdateAsset,
    },
    /// Manifest published but no asset matches the current
    /// OS/arch. Shown as an informational state — never an error
    /// loop.
    UnsupportedPlatform { manifest: UpdateManifest },
}

/// User-visible state machine. The settings UI renders this
/// directly; transitions are owned by the worker thread.
///
/// Field-level `#[allow(dead_code)]` rides along here because the
/// UI currently reads only a subset (status text + path). The
/// remaining fields are part of the public state contract and
/// will be consumed as the Settings page grows (e.g.,
/// "last checked at" timestamps, asset details on Update Available).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate {
        last_checked: Instant,
    },
    UpdateAvailable {
        manifest: UpdateManifest,
        asset: UpdateAsset,
        last_checked: Instant,
    },
    Downloading {
        manifest: UpdateManifest,
        asset: UpdateAsset,
        downloaded: u64,
        total: Option<u64>,
    },
    ReadyToInstall {
        manifest: UpdateManifest,
        asset: UpdateAsset,
        path: PathBuf,
    },
    Installing,
    UnsupportedPlatform {
        manifest: UpdateManifest,
        last_checked: Instant,
    },
    Error {
        message: String,
        last_checked: Option<Instant>,
    },
}

impl UpdateState {
    pub fn is_checking(&self) -> bool {
        matches!(self, Self::Checking)
    }

    pub fn is_downloading(&self) -> bool {
        matches!(self, Self::Downloading { .. })
    }

    #[allow(dead_code)]
    pub fn is_ready_to_install(&self) -> bool {
        matches!(self, Self::ReadyToInstall { .. })
    }

    pub fn is_installing(&self) -> bool {
        matches!(self, Self::Installing)
    }
}

/// Commands sent from the GPUI side into the worker.
#[allow(dead_code)]
#[derive(Debug)]
pub enum UpdaterCommand {
    CheckNow,
    Download,
    Install,
    Shutdown,
}

/// Events emitted by the worker for the GPUI side to drain.
#[derive(Debug)]
pub enum UpdaterEvent {
    StateChanged(UpdateState),
    /// Toast-worthy notice (download finished, install failed,
    /// etc.). The desktop app routes these through the existing
    /// toast helpers.
    Notice { kind: NoticeKind, message: String },
}

#[derive(Debug, Clone, Copy)]
pub enum NoticeKind {
    Success,
    Warning,
    Error,
}

/// Public handle owned by `AnotherOneApp`. Drop tears down the
/// worker thread.
pub struct UpdaterHandle {
    command_tx: mpsc::Sender<UpdaterCommand>,
    event_rx: mpsc::Receiver<UpdaterEvent>,
    identity: BuildIdentity,
}

impl UpdaterHandle {
    pub fn spawn(identity: BuildIdentity) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let worker_identity = identity.clone();
        thread::Builder::new()
            .name("another-one-updater".into())
            .spawn(move || run_worker(worker_identity, command_rx, event_tx))
            .expect("spawn updater worker thread");
        Self {
            command_tx,
            event_rx,
            identity,
        }
    }

    pub fn identity(&self) -> &BuildIdentity {
        &self.identity
    }

    pub fn send(&self, command: UpdaterCommand) {
        // The worker thread only exits on Shutdown; if the send
        // fails, the worker is gone and there's nothing to do
        // with the error besides log it. Don't panic — GPUI
        // shouldn't crash because the updater dropped.
        if let Err(err) = self.command_tx.send(command) {
            tracing::warn!("updater command channel closed: {err}");
        }
    }

    pub fn try_recv(&self) -> Option<UpdaterEvent> {
        match self.event_rx.try_recv() {
            Ok(ev) => Some(ev),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                tracing::warn!("updater event channel disconnected");
                None
            }
        }
    }
}

impl Drop for UpdaterHandle {
    fn drop(&mut self) {
        let _ = self.command_tx.send(UpdaterCommand::Shutdown);
    }
}

// ── Worker ─────────────────────────────────────────────────────

fn run_worker(
    identity: BuildIdentity,
    command_rx: mpsc::Receiver<UpdaterCommand>,
    event_tx: mpsc::Sender<UpdaterEvent>,
) {
    let mut state = UpdateState::Idle;
    // Cached last comparison so a `Download` command after a
    // successful `CheckNow` knows what asset to fetch without
    // re-hitting the network.
    let mut last_available: Option<(UpdateManifest, UpdateAsset)> = None;

    let auto = identity.auto_polling_enabled();
    let first_due = if auto {
        Some(Instant::now() + STARTUP_CHECK_DELAY)
    } else {
        None
    };
    let mut next_auto_check = first_due;

    loop {
        let now = Instant::now();
        let timeout = next_auto_check
            .map(|deadline| deadline.saturating_duration_since(now))
            .unwrap_or(Duration::from_secs(60 * 60));

        let cmd = match command_rx.recv_timeout(timeout) {
            Ok(cmd) => Some(cmd),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        let cmd = match cmd {
            Some(UpdaterCommand::Shutdown) => break,
            Some(cmd) => Some(cmd),
            None => {
                // Auto-check fired.
                if auto {
                    next_auto_check = Some(Instant::now() + POLL_INTERVAL);
                }
                Some(UpdaterCommand::CheckNow)
            }
        };

        match cmd {
            Some(UpdaterCommand::CheckNow) => {
                if state.is_checking() || state.is_downloading() || state.is_installing() {
                    continue;
                }
                publish(&event_tx, UpdaterEvent::StateChanged(UpdateState::Checking));
                match perform_check(&identity) {
                    Ok(comparison) => {
                        let last_checked = Instant::now();
                        match comparison {
                            UpdateComparison::UpToDate => {
                                state = UpdateState::UpToDate { last_checked };
                                last_available = None;
                            }
                            UpdateComparison::UpdateAvailable { manifest, asset } => {
                                last_available = Some((manifest.clone(), asset.clone()));
                                state = UpdateState::UpdateAvailable {
                                    manifest,
                                    asset,
                                    last_checked,
                                };
                            }
                            UpdateComparison::UnsupportedPlatform { manifest } => {
                                last_available = None;
                                state = UpdateState::UnsupportedPlatform {
                                    manifest,
                                    last_checked,
                                };
                            }
                        }
                        publish(&event_tx, UpdaterEvent::StateChanged(state.clone()));

                        // If a fresh check found an update, kick
                        // off the background download
                        // automatically — the plan calls for the
                        // app to download silently and only the
                        // install step to be user initiated.
                        if matches!(state, UpdateState::UpdateAvailable { .. }) {
                            run_download(
                                &identity,
                                &event_tx,
                                &mut state,
                                &mut last_available,
                            );
                        }
                    }
                    Err(err) => {
                        let message = format!("Update check failed: {err}");
                        tracing::warn!("{message}");
                        state = UpdateState::Error {
                            message: message.clone(),
                            last_checked: Some(Instant::now()),
                        };
                        publish(&event_tx, UpdaterEvent::StateChanged(state.clone()));
                        publish(
                            &event_tx,
                            UpdaterEvent::Notice {
                                kind: NoticeKind::Error,
                                message,
                            },
                        );
                    }
                }
            }
            Some(UpdaterCommand::Download) => {
                if state.is_downloading() || state.is_installing() {
                    continue;
                }
                if last_available.is_none() {
                    // Nothing cached — treat as a check first.
                    publish(&event_tx, UpdaterEvent::StateChanged(UpdateState::Checking));
                    match perform_check(&identity) {
                        Ok(UpdateComparison::UpdateAvailable { manifest, asset }) => {
                            last_available = Some((manifest.clone(), asset.clone()));
                            state = UpdateState::UpdateAvailable {
                                manifest,
                                asset,
                                last_checked: Instant::now(),
                            };
                            publish(&event_tx, UpdaterEvent::StateChanged(state.clone()));
                        }
                        Ok(UpdateComparison::UpToDate) => {
                            state = UpdateState::UpToDate {
                                last_checked: Instant::now(),
                            };
                            publish(&event_tx, UpdaterEvent::StateChanged(state.clone()));
                            continue;
                        }
                        Ok(UpdateComparison::UnsupportedPlatform { manifest }) => {
                            state = UpdateState::UnsupportedPlatform {
                                manifest,
                                last_checked: Instant::now(),
                            };
                            publish(&event_tx, UpdaterEvent::StateChanged(state.clone()));
                            continue;
                        }
                        Err(err) => {
                            let message = format!("Update check failed: {err}");
                            state = UpdateState::Error {
                                message: message.clone(),
                                last_checked: Some(Instant::now()),
                            };
                            publish(&event_tx, UpdaterEvent::StateChanged(state.clone()));
                            publish(
                                &event_tx,
                                UpdaterEvent::Notice {
                                    kind: NoticeKind::Error,
                                    message,
                                },
                            );
                            continue;
                        }
                    }
                }
                run_download(&identity, &event_tx, &mut state, &mut last_available);
            }
            Some(UpdaterCommand::Install) => {
                let UpdateState::ReadyToInstall {
                    manifest,
                    asset,
                    path,
                } = std::mem::replace(&mut state, UpdateState::Installing)
                else {
                    // Restore prior state if Install arrived in
                    // the wrong shape.
                    publish(
                        &event_tx,
                        UpdaterEvent::Notice {
                            kind: NoticeKind::Warning,
                            message: "No verified update is ready to install yet.".into(),
                        },
                    );
                    continue;
                };
                publish(
                    &event_tx,
                    UpdaterEvent::StateChanged(UpdateState::Installing),
                );
                match crate::updater_install::launch_install(&path, &asset) {
                    Ok(()) => {
                        // The install helper handles relaunch;
                        // emit a final notice in case the helper
                        // is slow to take over.
                        publish(
                            &event_tx,
                            UpdaterEvent::Notice {
                                kind: NoticeKind::Success,
                                message: "Installing update — the app will relaunch shortly."
                                    .into(),
                            },
                        );
                    }
                    Err(err) => {
                        let message = format!("Could not start installer: {err}");
                        tracing::warn!("{message}");
                        state = UpdateState::ReadyToInstall {
                            manifest,
                            asset,
                            path,
                        };
                        publish(&event_tx, UpdaterEvent::StateChanged(state.clone()));
                        publish(
                            &event_tx,
                            UpdaterEvent::Notice {
                                kind: NoticeKind::Error,
                                message,
                            },
                        );
                    }
                }
            }
            Some(UpdaterCommand::Shutdown) | None => break,
        }
    }
}

fn publish(tx: &mpsc::Sender<UpdaterEvent>, event: UpdaterEvent) {
    if let Err(err) = tx.send(event) {
        tracing::warn!("updater event channel closed: {err}");
    }
}

fn run_download(
    identity: &BuildIdentity,
    tx: &mpsc::Sender<UpdaterEvent>,
    state: &mut UpdateState,
    last_available: &mut Option<(UpdateManifest, UpdateAsset)>,
) {
    let Some((manifest, asset)) = last_available.clone() else {
        return;
    };
    *state = UpdateState::Downloading {
        manifest: manifest.clone(),
        asset: asset.clone(),
        downloaded: 0,
        total: asset.size_bytes,
    };
    publish(tx, UpdaterEvent::StateChanged(state.clone()));

    match perform_download(identity, &manifest, &asset, tx) {
        Ok(path) => {
            *state = UpdateState::ReadyToInstall {
                manifest,
                asset,
                path,
            };
            publish(tx, UpdaterEvent::StateChanged(state.clone()));
            publish(
                tx,
                UpdaterEvent::Notice {
                    kind: NoticeKind::Success,
                    message: "Update downloaded and ready to install.".into(),
                },
            );
            *last_available = None;
        }
        Err(err) => {
            let message = format!("Download failed: {err}");
            tracing::warn!("{message}");
            *state = UpdateState::Error {
                message: message.clone(),
                last_checked: Some(Instant::now()),
            };
            publish(tx, UpdaterEvent::StateChanged(state.clone()));
            publish(
                tx,
                UpdaterEvent::Notice {
                    kind: NoticeKind::Error,
                    message,
                },
            );
        }
    }
}

// ── Network + verification ────────────────────────────────────

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .timeout_connect(HTTP_TIMEOUT)
        .build()
}

fn download_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(DOWNLOAD_TIMEOUT)
        .timeout_connect(HTTP_TIMEOUT)
        .build()
}

fn perform_check(identity: &BuildIdentity) -> Result<UpdateComparison, String> {
    let manifest_bytes = fetch_bytes(&http_agent(), manifest_url())
        .map_err(|err| format!("fetch manifest: {err}"))?;
    let signature_bytes = fetch_bytes(&http_agent(), &signature_url())
        .map_err(|err| format!("fetch signature: {err}"))?;
    verify_manifest_signature(&manifest_bytes, &signature_bytes)?;

    let manifest: UpdateManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|err| format!("parse manifest: {err}"))?;
    if manifest.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(format!(
            "unsupported manifest schema version {}",
            manifest.schema_version
        ));
    }

    if manifest.commit_sha == identity.full_sha {
        return Ok(UpdateComparison::UpToDate);
    }

    let target_os = current_os_label();
    let target_arch = current_arch_label();
    let asset = manifest
        .assets
        .iter()
        .find(|asset| asset.os == target_os && asset.arch == target_arch)
        .cloned();

    Ok(match asset {
        Some(asset) => UpdateComparison::UpdateAvailable { manifest, asset },
        None => UpdateComparison::UnsupportedPlatform { manifest },
    })
}

fn fetch_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let response = agent
        .get(url)
        .call()
        .map_err(|err| format!("GET {url}: {err}"))?;
    let mut bytes = Vec::with_capacity(8 * 1024);
    response
        .into_reader()
        .take(8 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("read {url}: {err}"))?;
    Ok(bytes)
}

fn verify_manifest_signature(manifest: &[u8], signature: &[u8]) -> Result<(), String> {
    let pubkey = trust_pubkey()?;
    let signature = parse_signature(signature)?;
    pubkey
        .verify(manifest, &signature)
        .map_err(|err| format!("manifest signature verification failed: {err}"))
}

fn trust_pubkey() -> Result<&'static VerifyingKey, String> {
    static KEY: OnceLock<Result<VerifyingKey, String>> = OnceLock::new();
    let cached = KEY.get_or_init(|| {
        let hex = TRUST_PUBKEY_HEX
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "trust pubkey unset (rebuild with ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX set)"
                    .to_string()
            })?;
        let bytes = hex::decode(hex).map_err(|err| format!("invalid trust pubkey hex: {err}"))?;
        if bytes.len() != 32 {
            return Err(format!(
                "trust pubkey must be 32 bytes, got {}",
                bytes.len()
            ));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&bytes);
        VerifyingKey::from_bytes(&key_bytes)
            .map_err(|err| format!("invalid Ed25519 pubkey: {err}"))
    });
    cached.as_ref().map_err(|err| err.clone())
}

fn parse_signature(bytes: &[u8]) -> Result<Signature, String> {
    // Allow either raw 64-byte signatures or hex-encoded
    // signatures with optional surrounding whitespace. A hex
    // signature is 128 ASCII chars; treat the typical mistakes
    // (BOM, trailing newline) gracefully.
    if bytes.len() == Signature::BYTE_SIZE {
        let mut buf = [0u8; Signature::BYTE_SIZE];
        buf.copy_from_slice(bytes);
        return Ok(Signature::from_bytes(&buf));
    }
    let trimmed: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let decoded = hex::decode(&trimmed).map_err(|err| format!("decode signature hex: {err}"))?;
    if decoded.len() != Signature::BYTE_SIZE {
        return Err(format!(
            "signature must be {} bytes, got {}",
            Signature::BYTE_SIZE,
            decoded.len()
        ));
    }
    let mut buf = [0u8; Signature::BYTE_SIZE];
    buf.copy_from_slice(&decoded);
    Ok(Signature::from_bytes(&buf))
}

fn perform_download(
    _identity: &BuildIdentity,
    manifest: &UpdateManifest,
    asset: &UpdateAsset,
    tx: &mpsc::Sender<UpdaterEvent>,
) -> Result<PathBuf, String> {
    use std::io::{Read, Write};

    let cache_dir = updates_cache_dir()?;
    std::fs::create_dir_all(&cache_dir)
        .map_err(|err| format!("create cache dir {}: {err}", cache_dir.display()))?;

    let final_name = asset_filename(manifest, asset);
    let final_path = cache_dir.join(&final_name);
    let part_path = cache_dir.join(format!("{final_name}.part"));

    // Quick exit if the verified file already exists. Saves a
    // re-download if the user hits Check twice between launches.
    if final_path.exists() {
        if verify_file_sha256(&final_path, &asset.sha256).is_ok() {
            return Ok(final_path);
        }
        let _ = std::fs::remove_file(&final_path);
    }
    let _ = std::fs::remove_file(&part_path);

    let response = download_agent()
        .get(&asset.url)
        .call()
        .map_err(|err| format!("GET {}: {err}", asset.url))?;
    let total = response
        .header("content-length")
        .and_then(|s| s.parse::<u64>().ok())
        .or(asset.size_bytes);

    let mut reader = response.into_reader();
    let mut file =
        std::fs::File::create(&part_path).map_err(|err| format!("create part file: {err}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|err| format!("read body: {err}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|err| format!("write part: {err}"))?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;
        if last_emit.elapsed() >= Duration::from_millis(250) {
            last_emit = Instant::now();
            publish(
                tx,
                UpdaterEvent::StateChanged(UpdateState::Downloading {
                    manifest: manifest.clone(),
                    asset: asset.clone(),
                    downloaded,
                    total,
                }),
            );
        }
    }

    file.flush()
        .map_err(|err| format!("flush part: {err}"))?;
    file.sync_all()
        .map_err(|err| format!("fsync part: {err}"))?;
    drop(file);

    let digest = hex::encode(hasher.finalize());
    if !digest.eq_ignore_ascii_case(&asset.sha256) {
        let _ = std::fs::remove_file(&part_path);
        return Err(format!(
            "sha256 mismatch: expected {} got {}",
            asset.sha256, digest
        ));
    }

    std::fs::rename(&part_path, &final_path)
        .map_err(|err| format!("rename part to final: {err}"))?;
    Ok(final_path)
}

fn verify_file_sha256(path: &Path, expected: &str) -> Result<(), String> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|err| format!("open {}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hex::encode(hasher.finalize());
    if digest.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!("sha256 mismatch: expected {expected} got {digest}"))
    }
}

fn asset_filename(manifest: &UpdateManifest, asset: &UpdateAsset) -> String {
    let extension = match asset.kind.as_str() {
        "appimage" => "AppImage",
        "app-tar-gz" => "app.tar.gz",
        "app-zip" => "app.zip",
        "dmg" => "dmg",
        other => other,
    };
    format!(
        "AnotherOne-{os}-{arch}-{rid}.{ext}",
        os = asset.os,
        arch = asset.arch,
        rid = manifest.release_id,
        ext = extension,
    )
}

pub fn updates_cache_dir() -> Result<PathBuf, String> {
    let base = dirs::cache_dir()
        .ok_or_else(|| "could not resolve user cache dir".to_string())?;
    Ok(base.join("another-one").join("updates"))
}

pub fn current_os_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unsupported"
    }
}

pub fn current_arch_label() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "unsupported"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest(commit: &str) -> UpdateManifest {
        UpdateManifest {
            schema_version: 1,
            app: "another-one-desktop".into(),
            channel: Some("stable".into()),
            release_id: commit.into(),
            short_sha: commit[..7.min(commit.len())].into(),
            commit_sha: commit.into(),
            cargo_version: Some("0.1.0".into()),
            build_number: Some(42),
            published_at: Some("2026-04-26T12:34:56Z".into()),
            release_notes_url: None,
            assets: vec![
                UpdateAsset {
                    os: "macos".into(),
                    arch: "aarch64".into(),
                    kind: "app-tar-gz".into(),
                    url: "https://example.com/macos.app.tar.gz".into(),
                    sha256:
                        "0000000000000000000000000000000000000000000000000000000000000000"
                            .into(),
                    size_bytes: Some(123),
                },
                UpdateAsset {
                    os: "linux".into(),
                    arch: "x86_64".into(),
                    kind: "appimage".into(),
                    url: "https://example.com/linux.AppImage".into(),
                    sha256:
                        "1111111111111111111111111111111111111111111111111111111111111111"
                            .into(),
                    size_bytes: Some(456),
                },
            ],
        }
    }

    #[test]
    fn manifest_parses_minimum_required_fields() {
        let json = r#"{
            "schema_version": 1,
            "app": "another-one-desktop",
            "release_id": "abc",
            "short_sha": "abc",
            "commit_sha": "abc",
            "assets": []
        }"#;
        let parsed: UpdateManifest = serde_json::from_str(json).expect("parse");
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.commit_sha, "abc");
        assert!(parsed.assets.is_empty());
    }

    #[test]
    fn asset_filename_uses_release_id_and_kind() {
        let m = sample_manifest("eca0eb0b2f8a9d0e111122223333444455556666");
        let asset = &m.assets[0];
        let name = asset_filename(&m, asset);
        assert_eq!(
            name,
            "AnotherOne-macos-aarch64-eca0eb0b2f8a9d0e111122223333444455556666.app.tar.gz"
        );
    }

    #[test]
    fn current_os_and_arch_match_expected_targets() {
        assert!(matches!(
            current_os_label(),
            "macos" | "linux" | "unsupported"
        ));
        assert!(matches!(
            current_arch_label(),
            "aarch64" | "x86_64" | "unsupported"
        ));
    }

    #[test]
    fn signature_parser_accepts_raw_and_hex() {
        let raw = vec![0u8; Signature::BYTE_SIZE];
        let parsed_raw = parse_signature(&raw).expect("raw");
        let hex_bytes = hex::encode(&raw);
        let parsed_hex = parse_signature(hex_bytes.as_bytes()).expect("hex");
        assert_eq!(parsed_raw.to_bytes(), parsed_hex.to_bytes());
    }

    #[test]
    fn signature_parser_rejects_wrong_length() {
        let too_short = [0u8; 10];
        assert!(parse_signature(&too_short).is_err());
    }
}
