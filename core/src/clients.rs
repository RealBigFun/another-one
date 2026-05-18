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
use crate::project_store::{LinkedIssue, TaskKind};
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
    /// Optional external issue (GitHub, Jira, …) linked at creation.
    #[serde(default)]
    pub linked_issue: Option<LinkedIssue>,
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

/// Mobile (or any other "viewer") asks the daemon to make sure a
/// particular tab is live and start streaming its bytes. Different
/// shape from `OpenTab` — the tab already exists in the project
/// store (it was persisted by an earlier client); we're just
/// connecting a viewer to it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachTabRequest {
    pub client_id: ClientId,
    pub section_id: SectionId,
    pub tab_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachTabResponse {
    /// `true` if this call started the PTY; `false` if it was
    /// already running (idempotent attach).
    pub launched: bool,
}

/// Correlation token for an asynchronous open-task / open-worktree
/// flow. Worktree creation in particular runs in a background
/// thread (clone, branch checkout, project store insert) — the
/// caller gets a `JobId` immediately, and downstream events
/// (`TaskOpenStarted`, `TaskOpened`, `TaskOpenFailed`) carry the
/// same id so subscribers can correlate.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub String);

impl JobId {
    pub fn fresh() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
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
        /// The just-activated tab id, if a tab actually exists at
        /// emit time. Tasks created without a `launch_config` (e.g.
        /// "open the section, no terminal yet") emit with `None`
        /// rather than papering over the missing tab with an empty
        /// string — subscribers comparing against tab ids would
        /// otherwise be bitten by `tab_id == ""`.
        tab_id: Option<String>,
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
    /// PTY bytes streamed off a live tab. Emitted as the daemon
    /// reads chunks off the PTY — caller doesn't have to poll
    /// `read_terminal_output` to mirror content. No `originator`:
    /// the PTY isn't a client, and bytes carry their own causation.
    /// Volume note: a chatty TUI can blast the bus capacity (256
    /// events) faster than a slow consumer drains it; that consumer
    /// will see `try_recv` return `Lagged{skipped}` and should
    /// resync via `read_terminal_output`.
    Output { tab_id: String, bytes: Vec<u8> },
    /// An asynchronous open-task flow started — typically a
    /// worktree creation that's about to clone, branch, and load.
    /// `TaskOpened` (success) or `TaskOpenFailed` (error) follows,
    /// keyed by the same `job_id`.
    TaskOpenStarted {
        originator: ClientId,
        job_id: JobId,
        project_id: String,
    },
    /// Async open-task flow finished with an error before reaching
    /// `TaskOpened`. Subscribers should drop any pending state
    /// they were tracking under this `job_id`.
    TaskOpenFailed {
        originator: ClientId,
        job_id: JobId,
        error: String,
    },
    /// The session's `broadcast::Receiver` fell behind the bus's
    /// capacity and dropped `skipped` events before the next
    /// successful `recv`. Surfaces honestly so subscribers can
    /// resync (e.g. re-issue `list_tasks`/`list_tabs`) rather than
    /// silently drift. Synthesized client-side from
    /// `tokio::sync::broadcast::error::TryRecvError::Lagged`.
    Lagged { skipped: u64 },
}

// Note on shape: there is no `DaemonClient` trait here even though
// the request/response/event types in this file describe the verb
// surface clients use. Earlier drafts had one; it was deleted because
// the GUI side can't satisfy a sync `&self` trait method (every
// verb needs a GPUI `Context<Self>` for entity updates), so the
// trait would be implementable only by the off-process MCP /
// mobile drivers — which already implement
// `crate::mcp::orchestrator::McpOrchestrator`. A second parallel
// trait would be a nameless duplicate. The types alone are enough
// vocabulary to keep wire-compatible across drivers; the contract
// lives in `McpOrchestrator` for MCP and in matching method names
// (`client_open_task` / `_open_tab` / `_close_tab` / `_attach_tab`
// / `_select` / `_select_for`) on `AnotherOneApp` for the GUI.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn client_id_helpers_format_consistently() {
        assert_eq!(ClientId::gui_desktop().to_string(), "gui:desktop");
        assert_eq!(ClientId::mcp("claude-code").to_string(), "mcp:claude-code");
        // Mobile prefix truncates to a 12-char endpoint preview so
        // the bus tag stays short on noisy harness logs.
        assert_eq!(
            ClientId::mobile("c7f5664133e677efa468f89b27e9fb98").to_string(),
            "mobile:c7f5664133e6"
        );
    }

    #[test]
    fn focus_serde_round_trip() {
        let cases = vec![
            Focus::None,
            Focus::Project {
                project_id: "p".into(),
            },
            Focus::Task {
                project_id: "p".into(),
                task_id: "t".into(),
            },
            Focus::Tab {
                project_id: "p".into(),
                task_id: Some("t".into()),
                section_id: SectionId::for_task("p", "main", "t"),
                tab_id: "tab-1".into(),
            },
        ];
        for focus in cases {
            let wire = serde_json::to_value(&focus).expect("encode");
            let back: Focus = serde_json::from_value(wire.clone()).expect("decode");
            assert_eq!(back, focus, "round-trip mismatch for {:?}", wire);
        }
    }

    #[test]
    fn client_event_serde_covers_all_variants() {
        let variants: Vec<ClientEvent> = vec![
            ClientEvent::TaskOpened {
                originator: ClientId::gui_desktop(),
                task_id: "t".into(),
                section_id: SectionId::for_task("p", "main", "t"),
                tab_id: Some("tab".into()),
            },
            // Tasks created without a launch_config (e.g. an
            // explicitly-empty section) emit with `tab_id: None`
            // rather than papering over the missing tab.
            ClientEvent::TaskOpened {
                originator: ClientId::gui_desktop(),
                task_id: "t-empty".into(),
                section_id: SectionId::for_task("p", "main", "t-empty"),
                tab_id: None,
            },
            ClientEvent::Lagged { skipped: 17 },
            ClientEvent::TabOpened {
                originator: ClientId::mcp("h"),
                section_id: SectionId::new("p", "main"),
                tab_id: "tab".into(),
            },
            ClientEvent::TabClosed {
                originator: ClientId::gui_desktop(),
                tab_id: "tab".into(),
            },
            ClientEvent::FocusChanged {
                originator: ClientId::mcp("h"),
                target: ClientId::gui_desktop(),
                focus: Focus::None,
            },
            ClientEvent::Output {
                tab_id: "tab".into(),
                bytes: vec![0x68, 0x69],
            },
            ClientEvent::TaskOpenStarted {
                originator: ClientId::gui_desktop(),
                job_id: JobId("j".into()),
                project_id: "p".into(),
            },
            ClientEvent::TaskOpenFailed {
                originator: ClientId::gui_desktop(),
                job_id: JobId("j".into()),
                error: "boom".into(),
            },
        ];
        for ev in variants {
            let wire = serde_json::to_value(&ev).expect("encode");
            let back: ClientEvent = serde_json::from_value(wire.clone()).expect("decode");
            // Variants are externally tagged by serde default — the
            // top-level key matches the variant name.
            let key = wire
                .as_object()
                .and_then(|o| o.keys().next())
                .map(String::as_str)
                .unwrap_or("");
            assert!(
                !key.is_empty(),
                "encoded form not externally tagged: {wire}"
            );
            // Equality round-trips for every variant.
            let back_wire = serde_json::to_value(&back).expect("re-encode");
            assert_eq!(wire, back_wire);
        }
    }

    #[test]
    fn broadcast_bus_delivers_to_independent_subscribers() {
        // Models the daemon-side bus: one Sender, multiple per-
        // session Receivers. Both subscribers must observe every
        // event independently — no cross-session draining.
        let (tx, _) = tokio::sync::broadcast::channel(64);
        let mut a = tx.subscribe();
        let mut b = tx.subscribe();
        for i in 0..3 {
            tx.send(ClientEvent::TabClosed {
                originator: ClientId::mcp("test"),
                tab_id: format!("t{i}"),
            })
            .expect("send");
        }
        for rx in [&mut a, &mut b] {
            for i in 0..3 {
                let ev = rx.try_recv().expect("recv");
                match ev {
                    ClientEvent::TabClosed { tab_id, .. } => {
                        assert_eq!(tab_id, format!("t{i}"));
                    }
                    other => panic!("unexpected variant: {:?}", other),
                }
            }
            // After draining 3 events the receiver should be empty.
            assert!(matches!(
                rx.try_recv(),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
            ));
        }
    }

    #[test]
    fn open_task_request_warm_launch_hint_skipped_in_serde() {
        // GUI fast-path optimization — must not leak into MCP wire
        // format because non-GUI callers can't fabricate a valid
        // launch id.
        let req = OpenTaskRequest {
            client_id: ClientId::mcp("h"),
            project_id: "p".into(),
            task_name: None,
            branch_name: None,
            kind: crate::project_store::TaskKind::Direct,
            launch_config: TerminalLaunchConfig::default(),
            cwd: None,
            focus_after_open: true,
            warm_launch_hint: Some(42),
            linked_issue: None,
        };
        let wire = serde_json::to_value(&req).expect("encode");
        assert!(
            !wire.to_string().contains("warm_launch_hint"),
            "warm_launch_hint must be #[serde(skip)]: {wire}"
        );
        let back: OpenTaskRequest = serde_json::from_value(wire).expect("decode");
        assert_eq!(back.warm_launch_hint, None);
    }

    #[test]
    fn job_id_fresh_yields_unique_values() {
        // Sanity check — UUID4 collisions would corrupt the
        // TaskOpenStarted / TaskOpened correlation.
        let a = JobId::fresh();
        let b = JobId::fresh();
        assert_ne!(a, b);
        assert_eq!(a.0.len(), 36, "uuidv4 dashed length");
    }

    // Suppress the "imported but unused if no tests run" warning
    // for `json!` — referenced indirectly via serde_json macros.
    #[test]
    fn _serde_json_macro_anchor() {
        let _ = json!({"ok": true});
    }
}
