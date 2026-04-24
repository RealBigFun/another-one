//! The surface the daemon-hosted MCP server dispatches against.
//!
//! A concrete `McpOrchestrator` impl (lives in `desktop/`) bridges
//! the MCP tool calls to whatever actually owns tab/task state —
//! today that's `RegistryState` + `AnotherOneApp` through the same
//! pending-queue-drained-on-render-tick pattern `daemon_host.rs`
//! already uses for resize/launch requests.
//!
//! ## Threading
//!
//! All methods are **sync**. The MCP server runs in the daemon's
//! tokio runtime (see `desktop/src/daemon_host.rs`) and wraps calls
//! into this trait with `tokio::task::spawn_blocking` before
//! invoking. That lets impls take mutex locks or block on a
//! oneshot (for GPUI-thread round-trips) without starving the
//! reactor. Don't add `async` here — it would force every
//! GPUI-bridging impl to carry a runtime handle.
//!
//! ## Error semantics
//!
//! Read methods return `Option<T>` (None = the id doesn't exist)
//! rather than `Result`; looking up a missing task is not an error
//! worth bubbling to the MCP client. Write methods return
//! `anyhow::Result<T>` — an MCP `tools/call` maps `Err` to an
//! error response, `Ok` to a success response.
//!
//! ## Scope discipline
//!
//! No method here implies identity / auth. The daemon MCP is
//! observability-and-orchestration, not a security boundary (per
//! issues #34 and #35). Callers can ask for broader views than
//! their implied scope; enforcement is out of scope for Phase B+C.

use serde::{Deserialize, Serialize};

/// Read-only view of a project for `list_projects`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectInfo {
    pub id: String,
    pub path: String,
    pub label: String,
}

/// Read-only view of a task for `list_tasks`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskInfo {
    pub project_id: String,
    pub task_id: String,
    /// Branch name for worktree / multi-worktree tasks; `None` for
    /// direct tasks that don't own a branch.
    pub branch: Option<String>,
    /// Absolute path to the worktree for worktree tasks; `None`
    /// for direct tasks rooted at the project path.
    pub worktree_path: Option<String>,
}

/// Read-only view of a tab for `list_tabs`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TabInfo {
    pub tab_id: String,
    /// Provider string (matches `AgentProviderKind`'s serde name —
    /// e.g. `"claude-code"`, `"codex"`) for agent-backed tabs;
    /// `None` for plain shell tabs.
    pub provider: Option<String>,
    pub title: String,
    /// Agent session id if the tab is currently attached to one.
    pub session_ref: Option<String>,
}

/// Best-effort task status. Today we can only synthesise a coarse
/// `working` / `idle` distinction from tab-running state + recent
/// output activity. The richer state machine lives in issue #27;
/// when that lands, this enum and its producer extend together.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    /// At least one tab has an active PTY writer or has produced
    /// output recently.
    Working,
    /// All tabs present but none are producing output.
    Idle,
    /// Task exists but has no live tabs.
    NoTabs,
}

/// A snapshot of a tab's recent output. `bytes` is capped at the
/// caller-requested `tail` size (or the ring-buffer limit,
/// whichever is smaller). `truncated_head` is `true` when the
/// snapshot dropped older bytes to fit the request.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub bytes: Vec<u8>,
    pub truncated_head: bool,
}

/// `spawn_task` argument.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnTaskRequest {
    pub project_id: String,
    /// Provider string as it appears on `TabInfo::provider` (e.g.
    /// `"claude-code"`).
    pub harness: String,
    /// New branch name for worktree tasks; `None` means a direct
    /// task on the project's root.
    pub branch: Option<String>,
    pub initial_prompt: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnTaskResponse {
    pub project_id: String,
    pub task_id: String,
    pub worktree_path: Option<String>,
    pub tab_id: String,
}

/// `spawn_terminal` argument. Exactly one of `project_id` /
/// `task_id` must be set.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnTerminalRequest {
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnTerminalResponse {
    pub tab_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunCommandRequest {
    /// Exactly one of these identifies the target PTY.
    pub tab_id: Option<String>,
    pub task_id: Option<String>,
    /// Command as a raw string; the orchestrator writes this +
    /// `\n` to the PTY and waits for idle or timeout.
    pub command: String,
    /// Hard-capped at 5 minutes by the orchestrator regardless of
    /// what the caller asks for.
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunCommandResponse {
    pub output: Vec<u8>,
    /// True when the timeout fired before the PTY went idle. The
    /// PTY is not killed — callers can attach via
    /// `read_terminal_output` to continue observing.
    pub timed_out: bool,
}

/// Hard ceiling on `run_command` timeouts regardless of caller
/// request. A wedged harness must not hang the MCP session
/// indefinitely.
pub const RUN_COMMAND_TIMEOUT_CEILING_MS: u64 = 5 * 60 * 1_000;

/// The trait the MCP server dispatches against. Implementations
/// own whatever bridge is needed to reach the live app state
/// (today: `RegistryState` lookups for reads, pending-queue
/// enqueues for writes).
pub trait McpOrchestrator: Send + Sync {
    // ---- Read tools (#34) ----

    fn list_projects(&self) -> Vec<ProjectInfo>;

    fn list_tasks(&self) -> Vec<TaskInfo>;

    fn list_tabs(&self, task_id: &str) -> Vec<TabInfo>;

    fn get_task_status(&self, task_id: &str) -> Option<TaskStatus>;

    /// Return a snapshot of a tab's recent output. `tail` caps the
    /// number of bytes; if the underlying ring buffer is smaller
    /// than `tail`, the whole buffer is returned. Returns `None`
    /// if the tab doesn't exist.
    fn read_terminal_output(&self, tab_id: &str, tail: usize) -> Option<TerminalSnapshot>;

    // ---- Write tools (#35) ----

    fn spawn_task(&self, req: SpawnTaskRequest) -> anyhow::Result<SpawnTaskResponse>;

    fn spawn_terminal(&self, req: SpawnTerminalRequest) -> anyhow::Result<SpawnTerminalResponse>;

    fn send_input(&self, tab_id: &str, bytes: &[u8]) -> anyhow::Result<()>;

    fn run_command(&self, req: RunCommandRequest) -> anyhow::Result<RunCommandResponse>;

    fn close_tab(&self, tab_id: &str) -> anyhow::Result<()>;
}
