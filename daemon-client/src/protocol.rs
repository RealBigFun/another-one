//! Wire-protocol types — frame type tags, ALPN, `Control` (client → daemon),
//! `WorkerReply` (daemon → client), and the JSON shapes used inside
//! `WorkerReply::ProjectList`.
//!
//! Variants here MUST stay in lockstep with
//! `daemon/src/frame.rs`. Add a new variant on the daemon side
//! first, then mirror it here. The legacy `mobile-core` did the same
//! mirror-by-comment dance; we keep that pattern.

/// Must match the daemon's ALPN byte string. Bumped from `/0` → `/1`
/// when the daemon adopted the [`ControlEnvelope`] wrapper +
/// `Hello.protocol_version` handshake. Daemons on `/0` won't accept
/// us; clients on `/0` won't reach a modern daemon either (iroh
/// refuses the ALPN handshake before any stream opens).
pub const ALPN: &[u8] = b"anotherone/pty/1";

/// Wire protocol version, sent inside [`Control::Hello`]. Mismatch
/// closes the connection cleanly with `anotherone/incompatible-version`.
/// Keep in lockstep with `daemon::transport_iroh::PROTOCOL_VERSION`.
pub const PROTOCOL_VERSION: u32 = 1;

// Frame wire format, matching daemon/src/frame.rs:
//   [1 byte type][4 bytes BE length][N bytes payload]
pub const TY_DATA: u8 = 0x00;
pub const TY_CONTROL: u8 = 0x01;
pub const TY_WORKER_REPLY: u8 = 0x02;
/// See `daemon/src/frame.rs::MAX_FRAME_BYTES` for the rationale;
/// keep this value in lockstep with the daemon's cap.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Messages that can be sent via a type=1 control frame. Extend in lock-step
/// with `daemon/src/frame.rs::Control`.
///
/// Serialize-only: the client side doesn't need to decode control
/// frames (they're strictly client → daemon today).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    /// Legacy resize for the standalone sandbox shell. On the embedded
    /// (desktop-hosted) daemon, use [`Control::TabResize`] after
    /// [`Control::AttachTab`] — that routes the resize to the specific
    /// tab's PTY. Kept for backward compat with the smoke-test binary.
    Resize { cols: u16, rows: u16 },
    /// Ask the daemon to send back its current project list as a
    /// [`WorkerReply::ProjectList`] frame. Mirror of
    /// `daemon/src/frame.rs::Control::ListProjects`.
    ListProjects,
    /// Subscribe to the live PTY byte stream for `(section_id, tab_id)`.
    /// The daemon forwards the stream as a series of [`TY_DATA`] frames
    /// until the session closes or another `AttachTab` / `DetachTab`
    /// arrives — at most one attachment per session. Mirror of
    /// `daemon/src/frame.rs::Control::AttachTab`.
    AttachTab { section_id: String, tab_id: String },
    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent if nothing is attached. Mirror of
    /// `daemon/src/frame.rs::Control::DetachTab`.
    DetachTab,
    /// Resize the currently-attached tab's PTY. Silently no-ops when
    /// nothing is attached. Mirror of
    /// `daemon/src/frame.rs::Control::TabResize`.
    TabResize { cols: u16, rows: u16 },
    /// Ask the daemon to launch this tab's PTY if it's not already
    /// running. No-op if already live. Mirror of
    /// `daemon/src/frame.rs::Control::LaunchTab`.
    LaunchTab { section_id: String, tab_id: String },
    /// TOFU handshake — sent as the very first control frame after
    /// connect when this client has never paired with this daemon
    /// before. `pair_token` is the hex nonce parsed from the
    /// `pair=<hex>` query param on the pairing URL. `protocol_version`
    /// MUST equal [`PROTOCOL_VERSION`]; mismatch closes the connection
    /// with `anotherone/incompatible-version` before any other frames
    /// flow. Mirror of `daemon/src/frame.rs::Control::Hello`.
    Hello {
        pair_token: Option<String>,
        #[serde(default)]
        protocol_version: u32,
    },
}

/// Wire envelope for every type=1 control frame. Carries a
/// `request_id` so callers can correlate the daemon's reply against
/// the originating call without relying on stream ordering. Mirror of
/// `daemon/src/frame.rs::ControlEnvelope`. `#[serde(flatten)]` keeps
/// the on-wire JSON flat —
/// `{"request_id":17,"type":"hello","pair_token":...}` — matching the
/// daemon's parser.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ControlEnvelope {
    pub request_id: u64,
    #[serde(flatten)]
    pub control: Control,
}

/// `request_id == 0` is reserved for **push frames** the daemon
/// emits unsolicited (PTY bytes for an attached tab, broadcasts,
/// etc.). Clients MUST NOT use 0 as a request id when issuing calls.
pub const PUSH_REQUEST_ID: u64 = 0;

/// Daemon → client worker replies (type=2 frame payload, JSON). Mirror
/// of `daemon/src/frame.rs::WorkerReply`. Variants here are a curated
/// subset of what the daemon currently sends — unknown variants MUST
/// be ignored (`session.rs`'s recv loop peeks `kind` and drops anything
/// unrecognised) so a daemon newer than the client doesn't blow up the
/// connection.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerReply {
    /// Response to [`Control::ListProjects`]. Order matches the
    /// desktop sidebar.
    ProjectList { projects: Vec<ProjectSummary> },
}

/// Wire envelope for every type=2 worker-reply frame. `request_id`
/// matches the originating [`ControlEnvelope::request_id`], or
/// [`PUSH_REQUEST_ID`] (`0`) for daemon-pushed frames. Mirror of
/// `daemon/src/frame.rs::WorkerReplyEnvelope`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct WorkerReplyEnvelope {
    pub request_id: u64,
    #[serde(flatten)]
    pub reply: WorkerReply,
}

/// Mirror of `daemon/src/frame.rs::ProjectSummary`. Contains
/// the nested task + tab tree so one `ListProjects` response is enough
/// for the projects drawer + task page to render without follow-up
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

/// Mirror of `daemon/src/frame.rs::TaskSummary`. Carries the
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
    /// sort to the top of the projects drawer.
    pub pinned: bool,
}

/// Mirror of `daemon/src/frame.rs::TabSummary`. `running`
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
    /// pin glyph on the tab chip.
    pub pinned: bool,
    /// Matches `PersistedTerminalTab::fixed_title`. When `Some(_)`,
    /// render this instead of [`TabSummary::title`].
    pub fixed_title: Option<String>,
}

/// Mirror of `daemon/src/frame.rs::ProjectKind`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    Root,
    Worktree,
}

/// Mirror of `daemon/src/frame.rs::AgentProvider`. Wire form
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
