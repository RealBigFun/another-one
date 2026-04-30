//! Daemon-client vocabulary.
//!
//! Three actors drive the daemon today:
//! - **GUI** (the desktop GPUI window the user clicks in)
//! - **Mobile** (an iroh-paired phone)
//! - **MCP** (a harness or remote agent connected over the local UDS)
//!
//! Each one wants to do roughly the same things — open a task, attach a
//! tab, send keystrokes, focus a section, learn when the world changed
//! — but each has historically reached into the running app via its
//! own private path. That meant three implementations of "open a
//! terminal," each with their own subtle drift.
//!
//! This module defines the shared vocabulary. The data types (request /
//! response / event) live here so every driver speaks the same nouns.
//! The `DaemonClient` trait names the verbs — concrete impls live where
//! their execution context lives (GUI under GPUI on the main thread;
//! MCP on the daemon's tokio runtime; mobile inside the iroh handler).
//!
//! Event semantics: every state-changing call emits a `ClientEvent`.
//! Subscribers can replay or reflect remote actions — that's how the
//! MCP-elevated `select_for` lets a privileged client move another
//! client's focus, and how the GUI learns about MCP-driven changes so
//! it can render them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agents::TerminalLaunchConfig;
use crate::project_store::TaskKind;
use crate::section::SectionId;

/// Stable identifier for a daemon client. Strings rather than a typed
/// enum so new client kinds can be added without touching every match
/// arm; convention is `<kind>:<instance>` — `"gui:desktop"`,
/// `"mcp:claude-code-cli"`, `"mobile:<endpoint-id-prefix>"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(pub String);

impl ClientId {
    pub fn gui_desktop() -> Self {
        Self("gui:desktop".to_string())
    }

    pub fn mcp(handle: &str) -> Self {
        Self(format!("mcp:{handle}"))
    }

    pub fn mobile(endpoint: &str) -> Self {
        let short = endpoint.chars().take(12).collect::<String>();
        Self(format!("mobile:{short}"))
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a client currently has in front of it. The daemon tracks one
/// `Focus` per `ClientId`; privileged clients can read or set another
/// client's focus to drive demos / collaborative sessions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Focus {
    /// Nothing in particular. Default for fresh clients.
    None,
    /// The project page (no specific task / tab).
    Project { project_id: String },
    /// A task without a specific tab selected.
    Task { project_id: String, task_id: String },
    /// A specific tab. The most common "focus" for an active session.
    Tab {
        project_id: String,
        task_id: Option<String>,
        section_id: SectionId,
        tab_id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenTaskRequest {
    pub client_id: ClientId,
    pub project_id: String,
    /// Human-readable task name. None → daemon generates one
    /// (adjective-noun-adjective). Empty / whitespace also generates.
    pub task_name: Option<String>,
    /// Source branch to start from. None → daemon resolves the
    /// project's current branch.
    pub branch_name: Option<String>,
    pub kind: TaskKind,
    pub launch_config: TerminalLaunchConfig,
    /// Override for the spawned shell's cwd. None → use the project /
    /// task default working directory.
    pub cwd: Option<PathBuf>,
    /// If true, after the task is created the daemon also moves the
    /// caller's `Focus` onto the new tab. Drives the "MCP demo
    /// scrolls the user's view to the new tab" UX.
    pub focus_after_open: bool,
    /// GUI-internal hint: when the new-task modal prewarmed a PTY
    /// ahead of the user clicking submit, the warm launch id rides
    /// in here so the resulting tab attaches to that running PTY
    /// instead of starting a fresh one. Skipped over the wire — MCP
    /// and mobile clients always leave this `None`.
    #[serde(skip, default)]
    pub warm_launch_hint: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenTaskResponse {
    pub task_id: String,
    pub section_id: SectionId,
    pub tab_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenTabRequest {
    pub client_id: ClientId,
    pub section_id: SectionId,
    pub launch_config: TerminalLaunchConfig,
    pub focus_after_open: bool,
    /// GUI-internal hint, same semantics as `OpenTaskRequest.warm_launch_hint`.
    #[serde(skip, default)]
    pub warm_launch_hint: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenTabResponse {
    pub tab_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectRequest {
    pub client_id: ClientId,
    pub focus: Focus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloseTabRequest {
    pub client_id: ClientId,
    pub tab_id: String,
}

/// State change that any client can observe. Each event tracks the
/// `originator` so subscribers can ignore self-fired events (avoiding
/// echo loops) and surface remote-driven changes specifically.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientEvent {
    TaskOpened {
        originator: ClientId,
        task_id: String,
        section_id: SectionId,
        tab_id: String,
    },
    TabOpened {
        originator: ClientId,
        section_id: SectionId,
        tab_id: String,
    },
    TabClosed {
        originator: ClientId,
        tab_id: String,
    },
    FocusChanged {
        originator: ClientId,
        target: ClientId,
        focus: Focus,
    },
}

/// The shared verb surface. Concrete impls plug in for each driver
/// (GUI, MCP, mobile). Methods take `&self` rather than `&mut self`
/// because the impls usually mediate through interior mutability
/// (GPUI entity update / tokio message queue) — the trait stays
/// trivially boxable as `Arc<dyn DaemonClient>`.
pub trait DaemonClient: Send + Sync {
    fn id(&self) -> &ClientId;
    fn focus(&self) -> Focus;

    fn open_task(&self, req: OpenTaskRequest) -> anyhow::Result<OpenTaskResponse>;
    fn open_tab(&self, req: OpenTabRequest) -> anyhow::Result<OpenTabResponse>;
    fn close_tab(&self, req: CloseTabRequest) -> anyhow::Result<()>;
    fn send_input(&self, tab_id: &str, bytes: &[u8]) -> anyhow::Result<()>;
    fn select(&self, req: SelectRequest) -> anyhow::Result<()>;

    /// Subscribe to the global event stream. Implementations that
    /// can't surface events cheaply may return `None` — callers
    /// should treat that as "no observability available."
    fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<ClientEvent>>;
}

/// Elevated surface — clients capable of driving *other* clients. MCP
/// gets this so a connected harness can move the GUI's focus to the
/// tab it just spawned, observe what the user is doing on a peer
/// session, etc.
pub trait PrivilegedClient: DaemonClient {
    fn select_for(&self, target: ClientId, focus: Focus) -> anyhow::Result<()>;
    fn subscribe_for(
        &self,
        target: ClientId,
    ) -> Option<tokio::sync::broadcast::Receiver<ClientEvent>>;
}
