//! Simple length-prefixed framing for the Iroh bidi stream.
//!
//! Wire format: `[1 byte type][4 bytes BE length][N bytes payload]`.
//!
//! Types:
//! - `0x00` — PTY data (raw bytes, either direction)
//! - `0x01` — JSON control message (UTF-8; see [`Control`])
//! - `0x02` — JSON worker reply (UTF-8; see [`WorkerReply`]).
//!   Daemon → client only. One variant per core-extracted worker that
//!   the daemon forwards to the client. Unknown variants MUST be
//!   ignored by clients so older clients keep working as we add
//!   workers.
//!
//! Used by both the server ([`super::transport_iroh`]) and the client
//! smoke test (`bin/iroh-client.rs`). See [[docs/architecture/transport-abstraction]].

use anyhow::Context;
use serde::{Deserialize, Serialize};

pub const TY_DATA: u8 = 0x00;
pub const TY_CONTROL: u8 = 0x01;
pub const TY_WORKER_REPLY: u8 = 0x02;

/// Reject any frame larger than this. 64 KiB is comfortably more than
/// any real PTY chunk (readers use 4 KiB buffers) or resize JSON payload
/// (~40 bytes), so there is no legitimate reason for a peer to announce
/// a larger frame. Keeping the cap tight limits how much a compromised
/// paired peer can make the daemon allocate per frame.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Top-level envelope for every type=1 control frame. Carries a
/// `request_id` so the client can correlate the daemon's reply
/// against the originating call without relying on stream ordering
/// — once `ojm.2..8` land 20+ verbs flying in parallel, ordering
/// alone won't disambiguate.
///
/// Why an envelope rather than a `request_id` field on every
/// `Control` variant:
///   - `Control` already uses `#[serde(tag = "type")]` for its
///     variant discriminator. A separate envelope keeps the
///     correlation field out of the per-variant struct shape, so
///     adding a new domain variant in a sibling task is a one-line
///     change and serde's tag-flatten rules don't have to be
///     re-checked per variant.
///   - The wire cost is one extra `"request_id":N,` JSON pair per
///     frame — negligible against the 1-byte type + 4-byte length
///     header that already precedes the JSON.
///
/// `request_id == 0` is reserved for **push frames** the daemon
/// emits unsolicited (PTY bytes for an attached tab, future
/// project-tree refresh broadcasts, etc.). Clients MUST NOT use 0
/// as a request id when issuing calls — the dispatch table in the
/// Dart layer treats id 0 as "this is not a reply to anyone."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlEnvelope {
    pub request_id: u64,
    #[serde(flatten)]
    pub control: Control,
}

/// Top-level envelope for every type=2 worker-reply frame. Mirrors
/// [`ControlEnvelope`]: `request_id` matches the
/// `ControlEnvelope.request_id` of the call this is replying to,
/// or `0` for daemon-pushed frames that nobody asked for.
///
/// `#[serde(flatten)]` on `reply` keeps the on-wire JSON shape flat
/// — `{"request_id": 17, "kind": "project_list", "projects": [...]}`
/// — so the existing `serde(tag = "kind")` discriminator on
/// `WorkerReply` still works without nesting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerReplyEnvelope {
    pub request_id: u64,
    #[serde(flatten)]
    pub reply: WorkerReply,
}

/// Sentinel `request_id` value reserved for daemon-pushed
/// (unsolicited) frames. Clients filter on this rather than
/// matching against the request_id ↦ Completer table.
#[allow(dead_code)] // used by callers; the smoke-test bin compiles frame.rs in isolation
pub const PUSH_REQUEST_ID: u64 = 0;

/// Client → daemon session-control messages (type=1 frames). Payload
/// is JSON, wrapped in a [`ControlEnvelope`] that carries the
/// `request_id`. Server → client control is not currently used (the
/// daemon pushes data via `0x00` and worker replies via `0x02`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    /// Legacy resize for the standalone sandbox shell. On embedded
    /// (desktop-hosted) daemons, use [`Control::TabResize`] after
    /// [`Control::AttachTab`] — that routes the resize to the
    /// specific tab's PTY.
    Resize { cols: u16, rows: u16 },
    /// Legacy: ask the daemon to spawn `git_refresh` for a literal
    /// path. Preserved for backward compat with clients built before
    /// the projects/tasks/tabs protocol. New clients call
    /// [`Control::ListProjects`] then [`Control::AttachTab`].
    WatchProject { project_path: String },
    /// Ask the daemon to send its full project tree as a
    /// [`WorkerReply::ProjectList`] frame (projects → tasks → tabs).
    /// The embedded (desktop) daemon projects straight off the
    /// running `AnotherOneApp`; the standalone sandbox returns a
    /// synthetic tree with one task + one tab.
    ListProjects,
    /// Subscribe to the live PTY byte stream for `(section_id,
    /// tab_id)`. The daemon forwards the stream as a series of
    /// [`TY_DATA`] frames until either the session closes or
    /// another `AttachTab` / `DetachTab` arrives — at most one
    /// attachment per session.
    AttachTab { section_id: String, tab_id: String },
    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent if nothing is attached.
    DetachTab,
    /// Resize the currently-attached tab's PTY. Silently no-ops
    /// when nothing is attached.
    TabResize { cols: u16, rows: u16 },
    /// Ask the daemon to launch the task's tab as a live PTY if it
    /// isn't running. If already running, no-op. After this call,
    /// [`AttachTab`] will succeed. Both the desktop GUI and mobile
    /// are equal citizens in launching — neither is a "master" that
    /// gates the other.
    LaunchTab { section_id: String, tab_id: String },
    /// Snapshot of the host's "Open In" config — installed-and-enabled
    /// apps + the user's preferred default. Drives the mobile titlebar
    /// split-button's primary icon + the chevron dropdown. Reading it
    /// remotely is fine (display-only); the actual app launch
    /// (`open_project_in_app`) stays host-local — see the comment on
    /// `connection.dart::openProjectInApp` for why. Reply:
    /// [`WorkerReply::OpenInStateAck`].
    OpenInState,
    /// TOFU (trust-on-first-use) pairing handshake. Sent as the very
    /// first control frame by an unknown peer whose `NodeId` is NOT
    /// in the daemon's `paired_peers` allowlist. If the daemon's
    /// current pair nonce (regenerated at boot + on allowlist reset)
    /// matches `pair_token`, the peer's `NodeId` is appended to the
    /// allowlist, the nonce is consumed (cleared), and the session
    /// proceeds. Any mismatch closes the connection with
    /// `anotherone/unpaired`. Already-paired peers skip this frame
    /// entirely; sending it is a no-op for them.
    ///
    /// `pair_token` is the hex-encoded 128-bit nonce from the
    /// `pair=<hex>` query parameter on the pairing URL. A `None`
    /// (or missing) token from an unpaired peer is an
    /// unrecoverable rejection — we never auto-pair without proof
    /// the user scanned the current QR.
    ///
    /// `protocol_version` is the wire version the client speaks
    /// (see [`super::transport_iroh::PROTOCOL_VERSION`]). The daemon
    /// rejects mismatches with the
    /// `anotherone/incompatible-version` close reason instead of
    /// letting serde explode on the first unknown variant. Older
    /// (v0) daemons / clients on the previous ALPN won't reach this
    /// frame because iroh refuses the ALPN handshake before any
    /// stream opens — the in-band field is the belt-and-braces guard
    /// for any future transport (e.g. an iroh proxy that strips
    /// ALPN).
    ///
    /// `#[serde(default)]` lets a daemon decoding a Hello from an
    /// older client treat the missing field as `0` and surface the
    /// version mismatch cleanly rather than failing the decode
    /// itself.
    Hello {
        pair_token: Option<String>,
        #[serde(default)]
        protocol_version: u32,
    },
}

// ── Push vs pull contract for state mutations ────────────────────
//
// Foundation task `another-one-ojm.1` locks in the **inline-snapshot
// reply** model (option (a) of the two we considered):
//
//   Domain mutator replies — `ProjectAdded`, `TaskRenamed`,
//   `BranchCreated`, etc. — carry an inline `ProjectSummary`
//   (or scoped projection of it) so the issuing client updates
//   its tree from the reply directly. **No** separate
//   `ListProjects` round-trip is required after a successful
//   mutation.
//
// The rejected alternative (b) was: mutator replies return only an
// Ack, and the daemon pushes a fresh `WorkerReply::ProjectList` with
// `request_id == 0` to every connected client on every state change.
//
// Why (a):
//   - Single round-trip per mutation. Mobile-on-cellular cares.
//   - The mutating client's UI converges first (it gets the snapshot
//     synchronously with the reply). Other clients can still
//     subscribe to a separate push channel later — but adding that
//     subscription is purely additive over (a). Going the other way
//     (a → b) would be a wire break.
//   - Bandwidth: today the desktop is the only mutator and the
//     paired phone is the only other client. Pushing the full
//     `ProjectList` to every connected client on every state change
//     is wasted bytes for the typical 2-peer case. (a) limits the
//     post-mutation traffic to the issuer.
//   - Simpler daemon: no broadcast bookkeeping, no "which client
//     cares about project X" filter logic. Each `Control::*` mutator
//     handler emits one reply and is done.
//
// Domain children (`ojm.2..8`) follow this rule:
//   - Mutator verbs return a `WorkerReply::*` variant whose payload
//     contains the changed entity (or the full project summary if
//     the change cascades). Names like
//     `ProjectAdded { project: ProjectSummary }`,
//     `TaskRenamed { task: TaskSummary }`.
//   - Reader verbs return the projection the caller asked for, no
//     side-effects on other clients.
//   - If a future feature needs cross-client live updates (e.g.
//     "phone follows desktop's commit panel in real time"), it
//     lands as a separate opt-in `Control::Subscribe { topic }`
//     verb that pushes targeted `WorkerReply::*` frames with
//     `request_id == 0`. Today nothing in the GPUI desktop's UX
//     requires that, and YAGNI applies.

/// Worker replies (type=2 frames). Payload is JSON. Daemon → client
/// only.
///
/// Each variant is a lossy projection of one `core::*_service`
/// worker's reply type. We deliberately do *not* derive Serialize on
/// the core reply types themselves — those structs are shaped for the
/// desktop's GPUI state, with nested `Result<_, String>` and internal
/// metadata the mobile UI doesn't need. This wire type is the curated
/// subset we commit to as a public protocol.
///
/// Wire-compat rules:
/// - `#[serde(tag = "kind")]` — every message carries its discriminator,
///   so new variants can be added without renumbering.
/// - New variants: clients built before the variant existed hit
///   serde's "unknown variant" error. To stay forwards-compatible,
///   clients SHOULD decode into a shape that tolerates unknown
///   variants (e.g., decode to `serde_json::Value` first, then try
///   `WorkerReply`). The current Flutter client just logs-and-ignores
///   unknown frame *types* (via the `0x02` discriminator itself), so
///   until it upgrades to variant-awareness, the daemon should only
///   emit variants the contemporaneous client supports. Track client
///   capability out of band (ALPN version bump or a hello frame) when
///   we move beyond this slice.
/// - Mutators carry an inline state snapshot — see the "Push vs
///   pull" comment block immediately above.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerReply {
    /// Response to [`Control::ListProjects`]. Order matches the
    /// desktop sidebar's `project_order`; worktrees of a root are
    /// emitted as their own entries rather than nested children
    /// (the mobile UI can still group them by `repo_id` if it
    /// wants a tree rendering later).
    ProjectList { projects: Vec<ProjectSummary> },
    /// Reply to [`Control::OpenInState`]. `state.enabled_apps` is in
    /// canonical `OpenInAppKind::all()` order; `preferred_app_id`
    /// is `None` when no Open-In app is detected on the host.
    OpenInStateAck { state: OpenInStateWire },
    /// Uniform per-request failure frame. The daemon emits this in
    /// place of dropping the connection when a verb fails — keeps
    /// the channel open for other in-flight requests on the same
    /// session.
    ///
    /// `kind` is a small machine-classifiable enum (see [`ErrKind`])
    /// so clients can branch on the failure mode (retry on transient
    /// `internal`, surface auth UI on `unauthorised`, etc.) without
    /// pattern-matching on free-form `message` strings. `message`
    /// carries the human-readable detail and is logged / surfaced
    /// in toasts.
    ///
    /// Future domain children (`ojm.2..8`) emit `Err` instead of
    /// closing the connection on their own failure paths.
    Err {
        /// Pre-filled by `send_worker_reply`'s envelope wrapper, so
        /// callers don't have to thread it twice. Kept here for
        /// wire shape — payload is `{"kind":"err","request_id":N,"message":"...","err_kind":"..."}`
        /// after `#[serde(flatten)]` from `WorkerReplyEnvelope`.
        /// (Note the field name `err_kind` to avoid colliding with
        /// the envelope's outer `kind` discriminator.)
        message: String,
        #[serde(rename = "err_kind")]
        kind: ErrKind,
    },
}

/// Coarse classification of a daemon-side failure. Keep small —
/// callers branch on this in UI code, so adding a variant is a
/// commitment to render it. Most failures fall into `internal` (an
/// unexpected error worth logging) or `unsupported` (the daemon is
/// older than the client and doesn't know this verb).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrKind {
    /// The verb referenced an `id` (project/task/tab/section) the
    /// daemon doesn't recognise. Typically a stale client cache
    /// after the user removed something on another peer; clients
    /// should refresh their view rather than retrying the call.
    UnknownId,
    /// The daemon doesn't speak this verb yet — likely an older
    /// daemon paired with a newer client. The client can degrade
    /// gracefully (hide the offending UI affordance) until the
    /// host upgrades.
    Unsupported,
    /// The daemon recognises the verb but the calling peer isn't
    /// authorised to use it (e.g. read-only viewer trying to
    /// mutate). Reserved for the multi-peer authz model that
    /// lands after the foundation; today this is unreachable.
    Unauthorised,
    /// Any other failure — disk full, command spawn failed, git
    /// returned non-zero with stderr we don't classify. Treat as
    /// transient and retryable.
    Internal,
}

/// Lossy wire projection of `core::project_store::Project`, with
/// nested `tasks` + `tabs` so one `ListProjects` response tells the
/// mobile UI everything it needs to render its home drawer + each
/// project's task list without follow-up round-trips.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    /// Absolute path on the daemon host. Read-only on the wire —
    /// mobile never dereferences this, the desktop does all FS work.
    pub path: String,
    pub kind: ProjectKind,
    /// Last-observed current branch from the ProjectStore's
    /// `checkout.current_branch`; may be `None` if never read.
    pub current_branch: Option<String>,
    pub tasks: Vec<TaskSummary>,
}

/// Lossy wire projection of `core::project_store::Task`. Contains
/// enough for the mobile task page to render the tab strip and
/// request an attach; no live PTY state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub name: String,
    /// Stable section id — half of the compound
    /// `TerminalRuntimeKey { section_id, tab_id }` used to address
    /// a live PTY.
    pub section_id: String,
    pub branch_name: String,
    pub active_tab_id: String,
    pub tabs: Vec<TabSummary>,
    /// Desktop UI pins tasks via `UiState::pinned_task_ids` so they
    /// sort to the top of the sidebar; mirrored on mobile so the
    /// projects-drawer rendering matches.
    pub pinned: bool,
    /// Human-readable "5 minutes ago" string for the task's branch
    /// last commit. Populated from `branch.last_commit_relative` on
    /// the desktop's `ProjectStore`. Empty when the project hasn't
    /// been git-refreshed yet, so callers can join with `•` and
    /// drop empty segments. Wire-additive: older daemons will
    /// `serde(default)` this to `""`.
    #[serde(default)]
    pub last_commit_relative: String,
    /// Lines added on the task's working-tree branch since its
    /// merge base. Populated from `branch.lines_added` (only set
    /// when the branch is the worktree's current branch — by
    /// definition true for AnotherOne tasks). Wire-additive,
    /// defaults to `0`.
    #[serde(default)]
    pub lines_added: i32,
    /// Lines removed on the task's working-tree branch since its
    /// merge base. Wire-additive, defaults to `0`.
    #[serde(default)]
    pub lines_removed: i32,
    /// Project id this task targets for branch / Open-In / git
    /// actions. Equals `root_project_id` for plain tasks, points at
    /// the worktree's own `Project` entry for worktree tasks. The
    /// titlebar's "Open In" + Git Actions + Custom Actions all
    /// resolve their working directory through this id (matches
    /// `core::project_store::Task::target_project_id`). Wire-
    /// additive — older daemons leave it empty, in which case
    /// callers fall back to the root project id.
    #[serde(default)]
    pub target_project_id: String,
}

/// Lossy wire projection of
/// `core::project_store::PersistedTerminalTab`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabSummary {
    pub id: String,
    pub title: String,
    pub provider: Option<AgentProvider>,
    /// `true` iff the desktop has a live `LiveTerminalRuntime` for
    /// this tab right now. Persisted-but-not-launched tabs report
    /// `false` and an `AttachTab` for them returns no data.
    pub running: bool,
    /// User-pinned tabs stay resident across restarts on desktop;
    /// mobile shows a pin glyph on the chip and sorts them left.
    pub pinned: bool,
    /// User-overridden tab title. When `Some(_)`, prefer this over
    /// the auto-generated title field above (which tends to be the
    /// agent provider's default label).
    pub fixed_title: Option<String>,
}

/// Mirror of `core::project_store::ProjectKind`. Wire-serialised as
/// lowercase strings: `"root"` / `"worktree"`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    Root,
    Worktree,
}

// `PullRequestInfo` + `PullRequestState` removed along with the
// dead `WorkerReply::PullRequestStatus` variant. Reinstate when
// there's an actual PR-status emission site.

/// Mirror of `core::agents::AgentProviderKind`. Wire-serialised as
/// snake_case: `"claude_code"` / `"codex"` / `"cursor_agent"` etc.
/// `Shell` is the catch-all for tabs launched without an agent
/// provider set (plain PTY).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

/// Wire projection of `another_one_core::open_in::OpenInAppKind`
/// pre-hydrated with the display strings the mobile UI renders. The
/// daemon resolves them once at projection time so the wire payload
/// is one round-trip — mobile never needs to re-derive label/icon
/// from the id.
///
/// Field-for-field compatible with
/// `another_one_bridge::api::local_session::OpenInAppDto` so the
/// bridge can pass these straight through to its FRB-exposed DTO
/// without a mapping layer per field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenInAppWire {
    /// Stable id matching `OpenInAppKind::id()` — `"cursor"`,
    /// `"zed"`, `"vscode"`, `"file-manager"`.
    pub id: String,
    pub label: String,
    pub description: String,
    pub icon_path: String,
}

/// Wire projection of [`another_one_bridge::api::local_session::OpenInState`].
/// Mobile's titlebar uses `preferred_app_id` for its primary-action
/// icon and `enabled_apps` for the chevron dropdown. Actual app
/// launch stays host-local on the daemon (see `openProjectInApp`'s
/// docstring in `connection.dart`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenInStateWire {
    /// Apps offered in the dropdown, ordered as `OpenInAppKind::all()`
    /// declares them.
    pub enabled_apps: Vec<OpenInAppWire>,
    /// Id of the app the titlebar's primary action launches, or
    /// `None` when no app is enabled at all.
    pub preferred_app_id: Option<String>,
}

/// Reads one frame from an Iroh `RecvStream`. Returns `None` when the
/// peer has cleanly closed the send side.
pub async fn read_frame<R>(recv: &mut R) -> anyhow::Result<Option<(u8, Vec<u8>)>>
where
    R: ReadExactish + Unpin,
{
    let mut header = [0u8; 5];
    match recv.read_exactish(&mut header).await? {
        ReadOutcome::Closed => return Ok(None),
        ReadOutcome::Got => {}
    }
    let ty = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    anyhow::ensure!(
        len <= MAX_FRAME_BYTES,
        "frame too large: {len} bytes (max {MAX_FRAME_BYTES})"
    );
    let mut payload = vec![0u8; len];
    match recv.read_exactish(&mut payload).await? {
        ReadOutcome::Closed => anyhow::bail!("stream ended mid-frame"),
        ReadOutcome::Got => {}
    }
    Ok(Some((ty, payload)))
}

/// Writes one frame to an Iroh `SendStream`.
pub async fn write_frame<W>(send: &mut W, ty: u8, payload: &[u8]) -> anyhow::Result<()>
where
    W: WriteAllAsync + Unpin,
{
    let mut header = [0u8; 5];
    header[0] = ty;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    send.write_all_async(&header)
        .await
        .context("write header")?;
    send.write_all_async(payload)
        .await
        .context("write payload")?;
    Ok(())
}

// Tiny trait-shaped adapters so we can use these helpers with both iroh's
// send/recv streams and any future transport wrapper. Keeps the frame
// module transport-agnostic.

pub enum ReadOutcome {
    Got,
    Closed,
}

pub trait ReadExactish {
    fn read_exactish(
        &mut self,
        buf: &mut [u8],
    ) -> impl std::future::Future<Output = anyhow::Result<ReadOutcome>> + Send;
}

pub trait WriteAllAsync {
    fn write_all_async(
        &mut self,
        data: &[u8],
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

impl ReadExactish for iroh::endpoint::RecvStream {
    async fn read_exactish(&mut self, buf: &mut [u8]) -> anyhow::Result<ReadOutcome> {
        let mut read = 0;
        while read < buf.len() {
            match self.read(&mut buf[read..]).await {
                Ok(Some(0)) | Ok(None) => {
                    return if read == 0 {
                        Ok(ReadOutcome::Closed)
                    } else {
                        Err(anyhow::anyhow!(
                            "stream closed mid-read after {read} of {} bytes",
                            buf.len()
                        ))
                    };
                }
                Ok(Some(n)) => {
                    read += n;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(ReadOutcome::Got)
    }
}

impl WriteAllAsync for iroh::endpoint::SendStream {
    async fn write_all_async(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.write_all(data).await.map_err(Into::into)
    }
}
