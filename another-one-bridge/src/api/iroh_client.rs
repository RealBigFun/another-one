//! Iroh client exposed to Dart via flutter_rust_bridge.
//!
//! One `IrohSession` represents a live QUIC connection to a daemon that
//! speaks the `anotherone/pty/1` ALPN. Dart uses:
//!
//!   1. `iroh_connect(endpoint_id)` to dial.
//!   2. `session.send(bytes)` to deliver PTY input.
//!   3. `session.subscribe(sink)` to start receiving PTY output as a stream
//!      of `Vec<u8>` chunks.
//!   4. `session.close()` when finished.
//!
//! All iroh network work runs on a dedicated multi-thread tokio runtime
//! because FRB's default async executor is not a tokio runtime — iroh's
//! UDP sockets and internal actor tasks require tokio specifically, and
//! without this indirection `Endpoint::bind()` hangs forever on Android.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use anyhow::Context;
use flutter_rust_bridge::frb;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Mutex};

use crate::frb_generated::StreamSink;
use iroh::dns::DnsResolver;
use iroh::endpoint::presets;
use iroh::endpoint::{RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey};

/// Where we persist this device's iroh secret key. Set by Dart on app
/// start via [`set_data_dir`] — typically the application-support
/// directory (`getApplicationSupportDirectory()` on Android/iOS).
///
/// Without a stable secret key, each app launch yields a fresh
/// EndpointId, which breaks TOFU pairing — every restart would be
/// treated as a new peer and get rejected by the daemon. Persisting
/// the key keeps the phone's identity stable across app restarts,
/// reinstalls-with-backup, etc.
static DATA_DIR: std::sync::OnceLock<std::sync::Mutex<Option<PathBuf>>> =
    std::sync::OnceLock::new();

fn data_dir_slot() -> &'static std::sync::Mutex<Option<PathBuf>> {
    DATA_DIR.get_or_init(|| std::sync::Mutex::new(None))
}

/// Must match the daemon's ALPN byte string. Bumped to `/1` alongside
/// the introduction of `protocol_version` in `Control::Hello`,
/// `request_id` correlation, and the uniform `WorkerReply::Err`
/// frame. Version-suffixed so a future protocol break can run
/// `/2`-speakers in parallel without a flag day.
const ALPN: &[u8] = b"anotherone/pty/1";

/// In-band protocol version sent inside the first `Control::Hello`.
/// Mirror of `daemon_sandbox::transport_iroh::PROTOCOL_VERSION`.
const PROTOCOL_VERSION: u32 = 1;

// Frame wire format, matching daemon-sandbox/src/frame.rs:
//   [1 byte type][4 bytes BE length][N bytes payload]
const TY_DATA: u8 = 0x00;
const TY_CONTROL: u8 = 0x01;
const TY_WORKER_REPLY: u8 = 0x02;
/// See `daemon-sandbox/src/frame.rs::MAX_FRAME_BYTES` for the rationale;
/// keep this value in lockstep with the daemon's cap.
const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Top-level envelope for every type=1 control frame. Carries
/// `request_id` so the daemon's reply can be correlated against the
/// originating call from a `Map<u64, Completer<WorkerReply>>` on the
/// Dart side. Mirror of `daemon-sandbox/src/frame.rs::ControlEnvelope`.
#[derive(Debug, Clone, serde::Serialize)]
struct ControlEnvelope {
    request_id: u64,
    #[serde(flatten)]
    control: Control,
}

// Note: we deliberately don't define a `WorkerReplyEnvelope` mirror
// here. The recv loop decodes via `serde_json::Value` first so it can
// peek at the discriminator for forwards-compat (unknown variants
// from a newer daemon get logged-and-dropped). That two-stage decode
// already extracts `request_id` and `kind` separately, so a
// `#[serde(flatten)]` envelope struct would be unused weight.

/// Reserved `request_id` for unsolicited daemon → client frames
/// (PTY bytes, future project-tree refresh broadcasts, etc.).
/// Clients must not use `0` as a real request id when issuing calls.
pub(crate) const PUSH_REQUEST_ID: u64 = 0;

/// Messages that can be sent via a type=1 control frame. Extend in lock-step
/// with `daemon-sandbox/src/frame.rs::Control`.
///
/// Serialize-only: the Dart side doesn't need to decode control
/// frames (they're strictly client → daemon today).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    /// Legacy resize for the standalone sandbox shell. On the embedded
    /// (desktop-hosted) daemon, use [`Control::TabResize`] after
    /// [`Control::AttachTab`] — that routes the resize to the specific
    /// tab's PTY. Kept for backward compat with the smoke-test binary.
    Resize { cols: u16, rows: u16 },
    /// Ask the daemon to send back its current project list as a
    /// [`WorkerReply::ProjectList`] frame. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ListProjects`.
    ListProjects,
    /// Subscribe to the live PTY byte stream for `(section_id, tab_id)`.
    /// The daemon forwards the stream as a series of [`TY_DATA`] frames
    /// until the session closes or another `AttachTab` / `DetachTab`
    /// arrives — at most one attachment per session. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::AttachTab`.
    AttachTab { section_id: String, tab_id: String },
    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent if nothing is attached. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::DetachTab`.
    DetachTab,
    /// Resize the currently-attached tab's PTY. Silently no-ops when
    /// nothing is attached. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::TabResize`.
    TabResize { cols: u16, rows: u16 },
    /// Ask the daemon to launch this tab's PTY if it's not already
    /// running. No-op if already live. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::LaunchTab`.
    LaunchTab { section_id: String, tab_id: String },
    /// TOFU handshake — sent as the very first control frame after
    /// connect when this client has never paired with this daemon
    /// before. `pair_token` is the hex nonce parsed from the
    /// `pair=<hex>` query param on the pairing URL.
    /// `protocol_version` is the wire version we speak; the daemon
    /// closes with `anotherone/incompatible-version` on mismatch
    /// (see [`PROTOCOL_VERSION`]). Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::Hello`.
    Hello {
        pair_token: Option<String>,
        protocol_version: u32,
    },
    /// `another-one-ojm.5` — stage one changed file. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::StageChangedFile`.
    /// Reply is `WorkerReply::StageChangedFileAck` carrying the
    /// post-mutation `changed_files` snapshot.
    StageChangedFile {
        project_id: String,
        path: String,
        original_path: Option<String>,
    },
}

/// Daemon → client worker replies (type=2 frame payload, JSON). Mirror
/// of `daemon-sandbox/src/frame.rs::WorkerReply`; keep variants in
/// lockstep with the daemon's schema.
///
/// Each variant is a curated projection of one core worker's reply,
/// not a mechanical derive on the `core::*_service` reply structs.
/// That lets the daemon evolve its internal types freely and makes
/// the public wire schema a deliberate artifact.
///
/// FRB-exposed: passed to Dart as a tagged union inside
/// [`WorkerReplyMessage`] via the `subscribe_worker_replies` stream
/// on [`IrohSession`].
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerReply {
    /// Response to [`Control::ListProjects`]. Order matches the
    /// desktop sidebar. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ProjectList`.
    ProjectList { projects: Vec<ProjectSummary> },
    /// Uniform per-request failure frame. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::Err`. Domain
    /// callers in `ojm.2..8` map this to a Dart-level exception
    /// type so the UI can branch on `kind` without parsing
    /// `message` strings.
    Err {
        message: String,
        #[serde(rename = "err_kind")]
        kind: ErrKind,
    },
    /// `another-one-ojm.5` — ack for [`Control::StageChangedFile`].
    /// Mirror of `daemon-sandbox/src/frame.rs::WorkerReply::StageChangedFileAck`.
    /// Carries the post-mutation `changed_files` snapshot inline so
    /// the issuing client refreshes the right-sidebar Changes pane
    /// without a follow-up `ReadChangedFiles` round-trip.
    StageChangedFileAck { changed_files: Vec<ChangedFile> },
}

/// Mirror of `daemon-sandbox/src/frame.rs::ErrKind`. Wire form is
/// snake_case; the Dart side gets a freezed enum via FRB.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrKind {
    UnknownId,
    Unsupported,
    Unauthorised,
    Internal,
}

/// Pair of `(request_id, reply)` delivered to the Dart `IrohTransport`
/// over the `subscribe_worker_replies` stream. Splitting the
/// request_id out lets the Dart side maintain a
/// `Map<int, Completer<WorkerReply>>` keyed by request_id and complete
/// the matching future when the reply arrives, instead of relying on
/// stream-ordering for correlation.
///
/// `request_id == 0` (i.e. [`PUSH_REQUEST_ID`]) marks an unsolicited
/// daemon push that no caller is waiting on — the Dart layer routes
/// those to a separate broadcast subscription rather than the
/// completer table.
#[derive(Debug, Clone)]
pub struct WorkerReplyMessage {
    pub request_id: u64,
    pub reply: WorkerReply,
}

/// Mirror of `daemon-sandbox/src/frame.rs::ProjectSummary`. Contains
/// the nested task + tab tree so one `ListProjects` response is enough
/// for the mobile drawer + task page to render without follow-up
/// round-trips.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: ProjectKind,
    pub current_branch: Option<String>,
    pub tasks: Vec<TaskSummary>,
}

/// Mirror of `daemon-sandbox/src/frame.rs::TaskSummary`. Carries the
/// `section_id` half of the compound `TerminalRuntimeKey` used by
/// [`Control::AttachTab`].
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub name: String,
    pub section_id: String,
    pub branch_name: String,
    pub active_tab_id: String,
    pub tabs: Vec<TabSummary>,
    /// Mirrors desktop's `UiState::pinned_task_ids`. Pinned tasks
    /// sort to the top of the mobile projects drawer.
    pub pinned: bool,
    /// "5 minutes ago"-style string for the task's branch last
    /// commit. Empty when the project hasn't been git-refreshed
    /// yet — UI joins this with the branch name (when it differs
    /// from the task name) using `•` and drops empty segments,
    /// mirroring `desktop/src/left_sidebar.rs::branch_row`'s `meta`.
    #[serde(default)]
    pub last_commit_relative: String,
    /// Lines added on the task's working-tree branch since its
    /// merge base. UI renders `+N` in green next to the subtitle
    /// when non-zero. Mirrors GPUI's `branch.lines_added`.
    #[serde(default)]
    pub lines_added: i32,
    /// Lines removed on the task's working-tree branch since its
    /// merge base. UI renders `-N` in red next to `+N`.
    #[serde(default)]
    pub lines_removed: i32,
    /// Project id this task's working directory belongs to —
    /// root project id for plain tasks, the worktree's own project
    /// id for worktree tasks. The titlebar's Open-In / Git Actions
    /// / Custom Actions resolve their working dir through this id,
    /// so a worktree task opens its worktree path (not the root).
    /// Mirrors `core::project_store::Task::target_project_id`.
    #[serde(default)]
    pub target_project_id: String,
}

/// Mirror of `daemon-sandbox/src/frame.rs::TabSummary`. `running`
/// reflects whether the desktop has a live `LiveTerminalRuntime` for
/// this tab right now; `AttachTab` on a non-running tab yields no
/// data.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TabSummary {
    pub id: String,
    pub title: String,
    pub provider: Option<AgentProvider>,
    pub running: bool,
    /// Matches `PersistedTerminalTab::pinned`. Pinned tabs show a
    /// pin glyph on the mobile chip.
    pub pinned: bool,
    /// Matches `PersistedTerminalTab::fixed_title`. When `Some(_)`,
    /// render this instead of [`TabSummary::title`].
    pub fixed_title: Option<String>,
}

/// Mirror of `daemon-sandbox/src/frame.rs::ProjectKind`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    Root,
    Worktree,
}

/// Mirror of `daemon-sandbox/src/frame.rs::AgentProvider`. Wire form
/// is snake_case: `"claude_code"`, `"cursor_agent"`, `"codex"`, etc.
/// `Shell` is the catch-all for plain-PTY tabs with no agent
/// provider set.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProvider {
    ClaudeCode,
    CursorAgent,
    Codex,
    Pi,
    Gemini,
    OpenCode,
    Amp,
    RovoDev,
    Forge,
    Shell,
}

// `PullRequestInfo` + `PullRequestState` removed with the dead
// `WorkerReply::PullRequestStatus` variant on the daemon side.

/// Mirror of `daemon-sandbox/src/frame.rs::ChangedFile`. Carries the
/// post-mutation snapshot returned by `StageChangedFileAck` (and
/// future stage/unstage/discard acks landing in `another-one-ojm.5`).
/// Same field shape as `ChangedFileDto` on the FRB local-session
/// surface so the Dart layer can render the right-sidebar Changes
/// pane without re-projecting per transport.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub original_path: Option<String>,
    pub staged_additions: i32,
    pub staged_deletions: i32,
    pub unstaged_additions: i32,
    pub unstaged_deletions: i32,
    pub index_status: String,
    pub worktree_status: String,
    pub untracked: bool,
}

/// Writes one frame to the Iroh send stream.
async fn write_frame(send: &mut SendStream, ty: u8, payload: &[u8]) -> anyhow::Result<()> {
    let mut header = [0u8; 5];
    header[0] = ty;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    send.write_all(&header).await?;
    send.write_all(payload).await?;
    Ok(())
}

/// Reads one frame from the Iroh recv stream; returns `None` on clean EOF.
async fn read_frame(recv: &mut RecvStream) -> anyhow::Result<Option<(u8, Vec<u8>)>> {
    let mut header = [0u8; 5];
    let mut read = 0;
    while read < 5 {
        match recv.read(&mut header[read..]).await? {
            Some(0) | None => {
                return if read == 0 {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("stream ended mid-header"))
                };
            }
            Some(n) => read += n,
        }
    }
    let ty = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if len > MAX_FRAME_BYTES {
        anyhow::bail!("frame too large: {len} bytes");
    }
    let mut payload = vec![0u8; len];
    read = 0;
    while read < len {
        match recv.read(&mut payload[read..]).await? {
            Some(0) | None => anyhow::bail!("stream ended mid-payload"),
            Some(n) => read += n,
        }
    }
    Ok(Some((ty, payload)))
}

/// Load the device's persistent iroh secret key from
/// `{DATA_DIR}/iroh_secret_key`, or generate + write one on first
/// run. Fails if Dart hasn't called [`set_data_dir`] yet (meaning
/// we have nowhere safe to persist); in that case the caller is
/// expected to surface a clear error rather than silently falling
/// back to an ephemeral key that would break TOFU on restart.
fn load_or_create_device_secret_key() -> anyhow::Result<SecretKey> {
    let path = {
        let slot = data_dir_slot().lock().expect("data_dir mutex poisoned");
        slot.clone()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "set_data_dir must be called before iroh_connect — \
                     the app needs a persistent path for the device secret key"
                )
            })?
            .join("iroh_secret_key")
    };
    load_or_create_secret_key_at(&path)
}

fn load_or_create_secret_key_at(path: &Path) -> anyhow::Result<SecretKey> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create data dir {}", parent.display()))?;
    }
    if let Ok(content) = std::fs::read_to_string(path) {
        let trimmed = content.trim();
        let bytes = hex_decode_32(trimmed)
            .with_context(|| format!("parse secret key at {}", path.display()))?;
        return Ok(SecretKey::from_bytes(&bytes));
    }
    let sk = SecretKey::generate();
    let hex = hex_encode_32(&sk.to_bytes());
    std::fs::write(path, format!("{hex}\n"))
        .with_context(|| format!("write secret key to {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!(path = %path.display(), "generated new device iroh secret key");
    Ok(sk)
}

fn hex_encode_32(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn hex_decode_32(s: &str) -> anyhow::Result<[u8; 32]> {
    if s.len() != 64 {
        anyhow::bail!("expected 64 hex chars, got {}", s.len());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = u8::from_str_radix(&s[i * 2..i * 2 + 1], 16).context("bad hex")?;
        let lo = u8::from_str_radix(&s[i * 2 + 1..i * 2 + 2], 16).context("bad hex")?;
        *byte = (hi << 4) | lo;
    }
    Ok(out)
}

/// Dedicated tokio runtime for all iroh + local-session work. FRB's
/// default async executor is not a tokio runtime, so iroh's network
/// actors never get polled if we run them on the calling task — and
/// `LocalSession`'s subscription forwarders share the same need to
/// keep producing on a real runtime. `pub(crate)` so sibling
/// modules under `api/` can reuse it without each spinning up its
/// own runtime.
pub(crate) fn tokio_rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("another_one_bridge-tokio")
            .build()
            .expect("build tokio runtime")
    })
}

/// Record the application data directory Dart has chosen for us.
/// Must be called before `iroh_connect` so the secret key can be
/// loaded/created there. Safe to call multiple times — last write
/// wins. On Android/iOS, pass
/// `(await path_provider.getApplicationSupportDirectory()).path`.
pub fn set_data_dir(path: String) {
    let mut slot = data_dir_slot().lock().expect("data_dir mutex poisoned");
    *slot = Some(PathBuf::from(path));
}

#[frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
    setup_tracing();
    // Force the runtime to initialize eagerly so first-call latency doesn't
    // include runtime construction.
    let _ = tokio_rt();
}

/// Install a tracing subscriber that routes events to Android's logcat on
/// Android, and to stderr elsewhere. Default filter is modest; override with
/// `RUST_LOG` when debugging (e.g. `RUST_LOG=iroh=debug`).
fn setup_tracing() {
    use tracing_subscriber::{prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,another_one_bridge=info,iroh=warn"));

    #[cfg(target_os = "android")]
    let layer = tracing_android::layer("another_one_bridge").expect("tracing-android layer");

    #[cfg(not(target_os = "android"))]
    let layer = tracing_subscriber::fmt::layer();

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init();
}

/// Opaque handle to a live Iroh QUIC session. Dart holds this object and
/// calls methods on it; the actual Iroh state lives in Rust.
#[frb(opaque)]
pub struct IrohSession {
    /// The local endpoint we bound for this session. Closed on Drop.
    _endpoint: Endpoint,
    /// Sends framed messages (ty, payload) from Rust to the send task,
    /// which writes them into the QUIC send stream. `None` means closed.
    send_tx: Mutex<Option<mpsc::Sender<(u8, Vec<u8>)>>>,
    /// Holds the bytes-from-daemon stream until `subscribe()` wires it to a
    /// Dart `StreamSink`. Taken once and moved into the forwarding task.
    incoming_rx: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    /// Holds decoded worker replies (from `TY_WORKER_REPLY` frames) until
    /// `subscribe_worker_replies()` wires it to a Dart sink. Each item is
    /// a `(request_id, reply)` pair so the Dart layer can dispatch via
    /// its `Map<int, Completer<WorkerReply>>` rather than stream order.
    worker_replies_rx: Mutex<Option<mpsc::Receiver<WorkerReplyMessage>>>,
    /// Monotonic per-session request id allocator. Starts at 1 because
    /// `0` is reserved for daemon-pushed (unsolicited) frames — see
    /// [`PUSH_REQUEST_ID`]. Wraps at u64::MAX which is effectively
    /// never; even at 1 GHz issuance the wrap takes 500 years.
    next_request_id: AtomicU64,
    /// Closes the underlying connection when invoked.
    closer: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// Dial a daemon's Iroh endpoint by its public `EndpointId`.
///
/// At least one of `direct_addrs` or `relay_urls` must be non-empty — the
/// sandbox has no address-lookup service, so we can't discover how to reach
/// the daemon on our own. The daemon's ticket file prints both; pass them
/// through. When both are given iroh prefers the direct path and falls
/// back to the relay if hole-punching fails (the typical mobile-cellular
/// path).
pub async fn iroh_connect(
    endpoint_id: String,
    direct_addrs: Vec<String>,
    relay_urls: Vec<String>,
    pair_token: Option<String>,
) -> anyhow::Result<IrohSession> {
    tokio_rt()
        .spawn(async move {
            iroh_connect_inner(endpoint_id, direct_addrs, relay_urls, pair_token).await
        })
        .await
        .map_err(|e| anyhow::anyhow!("connect task panicked: {e}"))?
}

async fn iroh_connect_inner(
    endpoint_id: String,
    direct_addrs: Vec<String>,
    relay_urls: Vec<String>,
    pair_token: Option<String>,
) -> anyhow::Result<IrohSession> {
    tracing::info!(
        "iroh_connect: id={} direct={:?} relays={:?}",
        endpoint_id,
        direct_addrs,
        relay_urls,
    );

    let id: EndpointId = endpoint_id.trim().parse().context("invalid EndpointId")?;

    // Parse direct addresses eagerly so bad input surfaces before bind.
    let parsed_addrs: Vec<std::net::SocketAddr> = direct_addrs
        .iter()
        .map(|s| {
            s.parse::<std::net::SocketAddr>()
                .map_err(|e| anyhow::anyhow!("bad direct addr {s:?}: {e}"))
        })
        .collect::<anyhow::Result<_>>()?;
    let parsed_relays: Vec<RelayUrl> = relay_urls
        .iter()
        .map(|s| {
            s.parse::<RelayUrl>()
                .map_err(|e| anyhow::anyhow!("bad relay url {s:?}: {e}"))
        })
        .collect::<anyhow::Result<_>>()?;
    if parsed_addrs.is_empty() && parsed_relays.is_empty() {
        return Err(anyhow::anyhow!(
            "at least one direct address or relay URL is required \
             (sandbox has no address lookup)"
        ));
    }

    // Relay mode: if the caller gave us a relay URL, honour it (N0's dev
    // mesh lives behind `RelayMode::Default`). Otherwise stay disabled for
    // the LAN-only direct path.
    let relay_mode = if parsed_relays.is_empty() {
        RelayMode::Disabled
    } else {
        RelayMode::Default
    };
    tracing::info!(
        "iroh_connect: binding (Minimal preset, relay_mode={:?}, explicit DNS)",
        relay_mode,
    );
    // Android gotcha: `DnsResolver::default()` calls `with_system_defaults()`
    // which tries to read `/etc/resolv.conf`. iroh's own doc notes this "does
    // not work at least on some Androids" and says it falls back to Google
    // DNS — but in practice on the emulator the read hangs long enough to
    // stall bind(). We explicitly hand iroh a resolver so it skips system
    // detection entirely.
    //
    // Default is Cloudflare (`1.1.1.1:53`) rather than Google (`8.8.8.8:53`)
    // so every user's daemon lookups don't default to a Google-operated
    // resolver. Override with the `ANOTHERONE_DNS` env var if the user
    // wants a different provider — any `<ip>:<port>` string parseable as a
    // `SocketAddr` works. Fall back to the default silently on parse error
    // so a fat-fingered env var doesn't brick the mobile app.
    let dns_addr: std::net::SocketAddr = std::env::var("ANOTHERONE_DNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "1.1.1.1:53".parse().expect("static ipv4 socket addr"));
    tracing::info!(%dns_addr, "iroh_connect: using configured DNS resolver");
    let dns = DnsResolver::with_nameserver(dns_addr);
    // Persist the client's iroh identity so the EndpointId stays stable
    // across app restarts. Without this, TOFU pairing breaks every
    // time the user reopens the app.
    let secret_key =
        load_or_create_device_secret_key().context("load/create device iroh secret key")?;
    let endpoint = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Endpoint::builder(presets::Minimal)
            .secret_key(secret_key)
            .relay_mode(relay_mode)
            .alpns(vec![])
            .dns_resolver(dns)
            .bind(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("bind timed out after 15s (Minimal+DNS)"))?
    .context("bind client endpoint")?;
    tracing::info!("iroh_connect: endpoint bound, dialing {}", id);

    let mut addr = EndpointAddr::new(id);
    for sa in &parsed_addrs {
        addr = addr.with_ip_addr(*sa);
    }
    for url in parsed_relays {
        addr = addr.with_relay_url(url);
    }

    let conn = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        endpoint.connect(addr, ALPN),
    )
    .await
    .map_err(|_| anyhow::anyhow!("connect timed out after 10s"))?
    .context("connect to daemon")?;
    tracing::info!("iroh_connect: connected");

    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?;
    tracing::info!("iroh_connect: opened bidi stream");

    // Outbound pipe: Dart → channel → framed writes to Iroh send stream.
    // Channel items are already-framed (ty, payload) pairs so the writer
    // task doesn't need to know the protocol.
    let (send_tx, mut send_rx) = mpsc::channel::<(u8, Vec<u8>)>(64);
    // First frame MUST be `Control::Hello` so the daemon can complete
    // TOFU pairing before any other control / data frames arrive. The
    // daemon ignores Hello from already-paired peers, so sending it
    // unconditionally is safe. We send via the mpsc so ordering is
    // preserved with whatever the Dart layer sends next.
    //
    // Hello uses `request_id == PUSH_REQUEST_ID` (= 0) because no
    // caller is waiting on a reply — the daemon either accepts and
    // stays silent or closes the connection with
    // `anotherone/incompatible-version` / `anotherone/unpaired`.
    // Reserving 0 for "no reply expected" lets the Dart layer treat
    // any worker_reply with id 0 as a daemon push rather than a
    // dispatch-to-completer event.
    let hello_payload = serde_json::to_vec(&ControlEnvelope {
        request_id: PUSH_REQUEST_ID,
        control: Control::Hello {
            pair_token,
            protocol_version: PROTOCOL_VERSION,
        },
    })
    .context("encode hello")?;
    send_tx
        .send((TY_CONTROL, hello_payload))
        .await
        .map_err(|_| anyhow::anyhow!("send channel closed before hello"))?;
    tokio_rt().spawn(async move {
        while let Some((ty, payload)) = send_rx.recv().await {
            if let Err(e) = write_frame(&mut send, ty, &payload).await {
                tracing::debug!(error = %e, "iroh frame write failed");
                break;
            }
        }
        let _ = send.finish();
    });

    // Inbound pipe: framed reads from Iroh → per-frame-type channel → Dart
    // (once subscribed). Type=0 frames carry PTY output; type=2 frames carry
    // JSON-encoded `WorkerReply`s. Type=1 (server→client control) is
    // reserved for future use. Unknown types are logged and dropped so older
    // clients stay forwards-compatible as the daemon adds variants.
    let (incoming_tx, incoming_rx) = mpsc::channel::<Vec<u8>>(128);
    let (worker_replies_tx, worker_replies_rx) = mpsc::channel::<WorkerReplyMessage>(64);
    let (close_tx, mut close_rx) = tokio::sync::oneshot::channel::<()>();
    let conn_for_close = conn.clone();
    tokio_rt().spawn(async move {
        loop {
            tokio::select! {
                _ = &mut close_rx => break,
                frame = read_frame(&mut recv) => match frame {
                    Ok(Some((TY_DATA, payload))) => {
                        if incoming_tx.send(payload).await.is_err() {
                            break;
                        }
                    }
                    Ok(Some((TY_WORKER_REPLY, payload))) => {
                        // Two-stage decode for forwards-compat: parse
                        // as a generic JSON value first so we can
                        // peek at the `kind` discriminator. If the
                        // kind is one the current build knows, do
                        // the strict decode; otherwise log + drop so
                        // a future daemon variant (TaskStateChanged
                        // etc.) doesn't blow up an older client.
                        //
                        // The envelope is `#[serde(flatten)]`-ed onto
                        // `WorkerReply`, so the on-wire shape is one
                        // flat object: `{"request_id": N, "kind": "...", ...}`.
                        // We pluck `request_id` from the generic
                        // `Value` first (default 0 = push frame) then
                        // try the strict variant decode.
                        match serde_json::from_slice::<serde_json::Value>(&payload) {
                            Ok(value) => {
                                let request_id = value
                                    .get("request_id")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(PUSH_REQUEST_ID);
                                // Clone the discriminator before the
                                // strict decode moves `value` —
                                // otherwise we'd have no way to log
                                // the unknown variant name.
                                let kind = value
                                    .get("kind")
                                    .and_then(|k| k.as_str())
                                    .unwrap_or("<missing>")
                                    .to_string();
                                match serde_json::from_value::<WorkerReply>(value) {
                                    Ok(reply) => {
                                        let message = WorkerReplyMessage { request_id, reply };
                                        // try_send, not send().await — this
                                        // recv task also drives the PTY
                                        // stream which *does* want
                                        // backpressure; we can't let a
                                        // stuck worker_replies consumer
                                        // stall PTY bytes.
                                        use tokio::sync::mpsc::error::TrySendError;
                                        match worker_replies_tx.try_send(message) {
                                            Ok(()) => {}
                                            Err(TrySendError::Full(_)) => {
                                                tracing::debug!("worker_replies channel full; dropping frame");
                                            }
                                            Err(TrySendError::Closed(_)) => {
                                                tracing::debug!("worker_replies channel closed; dropping frame");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::debug!(
                                            kind,
                                            request_id,
                                            error = %e,
                                            "unknown/unsupported worker_reply variant; dropping (daemon is newer than client?)"
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    payload_bytes = payload.len(),
                                    "failed to parse worker_reply frame as JSON"
                                );
                            }
                        }
                    }
                    Ok(Some((ty, _))) => {
                        tracing::debug!(frame_type = ty, "unhandled iroh frame type");
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(error = %e, "iroh frame read failed");
                        break;
                    }
                },
            }
        }
        conn_for_close.close(0u8.into(), b"close");
    });

    Ok(IrohSession {
        _endpoint: endpoint,
        send_tx: Mutex::new(Some(send_tx)),
        incoming_rx: Mutex::new(Some(incoming_rx)),
        worker_replies_rx: Mutex::new(Some(worker_replies_rx)),
        // Start at 1: id 0 is reserved for daemon-pushed frames so
        // the Dart layer can distinguish "reply to my call" from
        // "unsolicited update" without inspecting variant kinds.
        next_request_id: AtomicU64::new(1),
        closer: Mutex::new(Some(close_tx)),
    })
}

impl IrohSession {
    /// Send raw bytes to the daemon (will be written into the PTY's stdin).
    pub async fn send(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.send_frame(TY_DATA, bytes).await
    }

    /// Allocate the next per-session request id. Dart calls this
    /// before issuing a control verb so it can register a `Completer`
    /// in its dispatch map keyed by the same id. Strictly-monotonic
    /// across the session; never returns 0 (reserved for push
    /// frames — see [`PUSH_REQUEST_ID`]).
    pub fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Request a PTY resize on the daemon's end. Goes through the same
    /// stream as data, multiplexed by frame type. The legacy `Resize`
    /// variant carries no data the client needs to wait on, so it
    /// uses a fresh request_id but no caller correlates against it.
    pub async fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.send_control(self.next_request_id(), Control::Resize { cols, rows })
            .await
    }

    /// Ask the daemon to send back its current project list as a
    /// [`WorkerReply::ProjectList`] frame. The reply arrives on
    /// `subscribe_worker_replies` with a matching `request_id`;
    /// today the Dart wrapper still consumes by stream order, so the
    /// id round-trips but isn't dispatched-on yet — domain tasks
    /// (`another-one-ojm.2..8`) are responsible for migrating each
    /// verb to the completer-table model.
    pub async fn list_projects(&self) -> anyhow::Result<()> {
        self.send_control(self.next_request_id(), Control::ListProjects)
            .await
    }

    /// Subscribe this session to the live PTY byte stream for
    /// `(section_id, tab_id)`. The daemon will forward the attached
    /// tab's output as [`TY_DATA`] frames on the existing `subscribe`
    /// sink. At most one attachment per session — re-issuing replaces
    /// the previous one. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::AttachTab`.
    pub async fn attach_tab(&self, section_id: String, tab_id: String) -> anyhow::Result<()> {
        self.send_control(
            self.next_request_id(),
            Control::AttachTab { section_id, tab_id },
        )
        .await
    }

    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent if nothing is attached. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::DetachTab`.
    pub async fn detach_tab(&self) -> anyhow::Result<()> {
        self.send_control(self.next_request_id(), Control::DetachTab)
            .await
    }

    /// Resize the currently-attached tab's PTY. Silently no-ops on
    /// the daemon when nothing is attached. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::TabResize`.
    pub async fn tab_resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.send_control(self.next_request_id(), Control::TabResize { cols, rows })
            .await
    }

    /// Ask the daemon to launch the tab's PTY if it isn't already
    /// live. No-op on the daemon side if the tab is already running.
    /// After this, a subsequent `attach_tab` will receive bytes.
    pub async fn launch_tab(&self, section_id: String, tab_id: String) -> anyhow::Result<()> {
        self.send_control(
            self.next_request_id(),
            Control::LaunchTab { section_id, tab_id },
        )
        .await
    }

    /// `another-one-ojm.5` — issue a `Control::StageChangedFile`
    /// frame against the daemon. Fire-and-forget at the FRB level:
    /// the matching `WorkerReply::StageChangedFileAck` arrives on
    /// `subscribe_worker_replies` keyed to a fresh `request_id` the
    /// Dart layer allocates via [`Self::next_request_id`]. The Dart
    /// `IrohTransport` registers a `Completer` against that id
    /// before calling, so the await-side awaits the ack from there.
    pub async fn stage_changed_file(
        &self,
        request_id: u64,
        project_id: String,
        path: String,
        original_path: Option<String>,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::StageChangedFile {
                project_id,
                path,
                original_path,
            },
        )
        .await
    }

    /// Wrap a `Control` in the `request_id`-tagged envelope and push
    /// it to the writer task. Internal to the existing per-verb
    /// helpers above; future per-verb additions in `ojm.2..8` can
    /// also reuse this rather than re-implementing the wrap.
    async fn send_control(&self, request_id: u64, control: Control) -> anyhow::Result<()> {
        let payload = serde_json::to_vec(&ControlEnvelope {
            request_id,
            control,
        })
        .context("encode control envelope")?;
        self.send_frame(TY_CONTROL, payload).await
    }

    async fn send_frame(&self, ty: u8, payload: Vec<u8>) -> anyhow::Result<()> {
        let tx = self.send_tx.lock().await;
        match tx.as_ref() {
            Some(tx) => tx
                .send((ty, payload))
                .await
                .map_err(|_| anyhow::anyhow!("send channel closed")),
            None => Err(anyhow::anyhow!("session closed")),
        }
    }

    /// Start pushing inbound bytes into the given Dart StreamSink. Call once
    /// per session; subsequent calls return an error.
    pub async fn subscribe(&self, sink: StreamSink<Vec<u8>>) -> anyhow::Result<()> {
        let mut guard = self.incoming_rx.lock().await;
        let mut rx = guard
            .take()
            .ok_or_else(|| anyhow::anyhow!("already subscribed"))?;
        drop(guard);

        tokio_rt().spawn(async move {
            while let Some(bytes) = rx.recv().await {
                if sink.add(bytes).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    /// Start pushing decoded worker replies into the given Dart StreamSink.
    /// Same one-shot subscription shape as [`subscribe`]; the second call
    /// returns an error. Each item is a [`WorkerReplyMessage`] carrying
    /// the originating `request_id` (or [`PUSH_REQUEST_ID`] = `0` for
    /// daemon-pushed frames) plus the decoded variant. Replies arrive
    /// in the order the daemon sent them; the Dart layer dispatches
    /// against its `Map<int, Completer<WorkerReply>>` rather than
    /// relying on ordering.
    pub async fn subscribe_worker_replies(
        &self,
        sink: StreamSink<WorkerReplyMessage>,
    ) -> anyhow::Result<()> {
        let mut guard = self.worker_replies_rx.lock().await;
        let mut rx = guard
            .take()
            .ok_or_else(|| anyhow::anyhow!("already subscribed to worker replies"))?;
        drop(guard);

        tokio_rt().spawn(async move {
            while let Some(message) = rx.recv().await {
                if sink.add(message).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    /// Closes the session. Safe to call multiple times.
    pub async fn close(&self) {
        self.send_tx.lock().await.take();
        if let Some(close_tx) = self.closer.lock().await.take() {
            let _ = close_tx.send(());
        }
    }
}
