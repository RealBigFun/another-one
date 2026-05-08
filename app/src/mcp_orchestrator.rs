//! `McpOrchestrator` implementation for the embedded daemon.
//!
//! Projects [`AnotherOneApp`] state (via the shared `RegistryState`
//! already used by `DesktopTerminalRegistry`) onto the MCP tool
//! surface. All methods are sync; the daemon's tokio runtime
//! enters them via `spawn_blocking` / `block_in_place` as needed
//! (the UDS transport layer in `daemon-sandbox` already does
//! this).
//!
//! ## Broadcast audit (per the unification plan)
//!
//! The `Control::*` dispatch surface persists+broadcasts via
//! `DesktopTerminalRegistry::with_store_mut`. MCP doesn't go through
//! `with_store_mut` — its mutating calls enqueue onto
//! `RegistryState.pending_*` queues, and the GPUI render tick drains
//! those queues into `client_open_task` / `client_open_tab` /
//! `client_close_tab` / `dispatch_ui_action`. Each drain handler
//! ends with `self.sync_registry_project_store()` (`app.rs:6674`,
//! `:6757`, `:6914`), which fires the same `state_change_tx`
//! broadcast as `with_store_mut`. So MCP-driven changes reach mobile
//! clients via the same push pump.
//!
//! Per-method audit:
//!
//! | method                | writes store? | broadcasts? | verdict |
//! | --------------------- | ------------- | ----------- | ------- |
//! | `list_projects`       | no            | n/a         | OK      |
//! | `list_tasks`          | no            | n/a         | OK      |
//! | `list_tabs`           | no            | n/a         | OK      |
//! | `get_task_status`     | no            | n/a         | OK      |
//! | `read_terminal_output`| no            | n/a         | OK      |
//! | `spawn_task`          | TODO          | TODO        | not-yet-wired (returns error) |
//! | `spawn_terminal`      | yes (via render tick) | yes (`sync_registry_project_store` at end of `fulfill_spawn_terminal` → `client_open_task`/`client_open_tab`) | OK |
//! | `send_input`          | no — PTY only | n/a         | OK      |
//! | `run_command`         | TODO          | TODO        | not-yet-wired (returns error) |
//! | `close_tab`           | yes (via render tick) | yes (`sync_registry_project_store` in `client_close_tab`) | OK |
//! | `select_focus`        | no — desktop-ephemera focus only | n/a | OK (mobile doesn't render desktop focus) |
//! | `dispatch_ui_action`  | no — overlay/zoom flags only | n/a | OK |
//! | `subscribe_events`    | no — read-only event stream | n/a | OK |
//!
//! No MCP write path bypasses the broadcast today. If a future MCP
//! tool mutates `project_store` outside the existing
//! `client_open_*` / `client_close_*` / `with_store_mut` paths, it
//! must call `sync_registry_project_store` (or
//! `state.notify_state_changed()`) post-persist or mobile clients
//! won't see the change.
//!
//! ## What's wired today (Phase B + partial C)
//!
//! **Read tools**: all of them (`list_projects`, `list_tasks`,
//! `list_tabs`, `get_task_status`, `read_terminal_output`) —
//! answered directly from `RegistryState` without ever
//! posting work back to the GPUI thread.
//!
//! **Write tools**: only `send_input` is fully functional today
//! (it reuses the same writer-Arc path as `DesktopTerminalRegistry::tab_input`).
//! The other four (`spawn_task`, `spawn_terminal`, `run_command`,
//! `close_tab`) need GPUI-thread access to launch flows and tab
//! closure; they're wired to a pending-queue pattern that the
//! render tick will drain in a follow-up PR. Today they return
//! a clear "not yet wired" error so MCP clients get a usable
//! signal rather than a silent no-op.

use std::sync::{Arc, Mutex, Weak};

use another_one_core::agents::AgentProviderKind;
use another_one_core::mcp::orchestrator::{
    McpOrchestrator, ProjectInfo, RunCommandRequest, RunCommandResponse, SpawnTaskRequest,
    SpawnTaskResponse, SpawnTerminalRequest, SpawnTerminalResponse, TabInfo, TaskInfo, TaskStatus,
    TerminalSnapshot,
};
use another_one_core::project_store::ProjectKind as CoreProjectKind;
use another_one_core::section::SectionId;

use crate::daemon_host::RegistryState;
use crate::terminal_runtime::TerminalRuntimeKey;

pub(crate) struct DesktopMcpOrchestrator {
    inner: Weak<Mutex<RegistryState>>,
    /// Direct handle on the daemon-side `ClientEvent` bus so MCP
    /// session-subscribe doesn't have to round-trip through the
    /// registry mutex. Cloned from `AnotherOneApp.event_bus` at
    /// orchestrator construction.
    event_bus: tokio::sync::broadcast::Sender<another_one_core::clients::ClientEvent>,
}

impl DesktopMcpOrchestrator {
    pub(crate) fn new(
        inner: Weak<Mutex<RegistryState>>,
        event_bus: tokio::sync::broadcast::Sender<another_one_core::clients::ClientEvent>,
    ) -> Self {
        Self { inner, event_bus }
    }

    fn with_state<R>(&self, f: impl FnOnce(&RegistryState) -> R) -> Option<R> {
        let arc = self.inner.upgrade()?;
        let guard = arc.lock().ok()?;
        Some(f(&*guard))
    }
}

/// Returned by `spawn_task` and `run_command` until those tools are
/// wired through render-tick drains parallel to `spawn_terminal` /
/// `close_tab` / `select_focus`. The daemon-side surface (request /
/// response types in `core/src/mcp/orchestrator.rs`) is final; the
/// pending-queue + drain pair on the desktop is what's missing.
///
/// `spawn_task` needs an agent harness chooser plus the warm-prewarm
/// path the GUI's add-agent-modal uses; `run_command` needs idle
/// detection (5-min ceiling already enforced wire-side) and PTY
/// output capture as it streams. Both should return promptly with
/// a `JobId`; correlated `TaskOpened` / `Output` / completion events
/// already exist on the bus, so the verb shape is "queue, return,
/// observe via `poll_events`."
const NOT_YET_WIRED: &str =
    "this MCP write tool is not yet wired to the desktop's UI-thread task/terminal lifecycle \
     (tracked as a Phase C.5 follow-up to #35 — daemon-side surface is in place, app-side \
     drains are not)";

impl McpOrchestrator for DesktopMcpOrchestrator {
    // ---- Read tools (#34) ----

    fn list_projects(&self) -> Vec<ProjectInfo> {
        self.with_state(|state| {
            state
                .project_store
                .projects
                .iter()
                .filter(|p| matches!(p.kind, CoreProjectKind::Root))
                .map(|p| ProjectInfo {
                    id: p.id.clone(),
                    path: p.path.to_string_lossy().into_owned(),
                    label: p.name.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn list_tasks(&self) -> Vec<TaskInfo> {
        self.with_state(|state| {
            let store = &state.project_store;
            let mut out = Vec::new();
            for project in store
                .projects
                .iter()
                .filter(|p| matches!(p.kind, CoreProjectKind::Root))
            {
                let Some(tasks) = store.tasks.get(&project.id) else {
                    continue;
                };
                for task in tasks {
                    // Worktree tasks carry a non-empty `branch_name`
                    // and a `worktree_project_id` pointing at the
                    // separate worktree project we created for them.
                    let worktree_path = task
                        .worktree_project_id
                        .as_ref()
                        .and_then(|id| store.projects.iter().find(|p| p.id == *id))
                        .map(|wt| wt.path.to_string_lossy().into_owned());
                    out.push(TaskInfo {
                        project_id: project.id.clone(),
                        task_id: task.id.clone(),
                        branch: if task.branch_name.is_empty() {
                            None
                        } else {
                            Some(task.branch_name.clone())
                        },
                        worktree_path,
                    });
                }
            }
            out
        })
        .unwrap_or_default()
    }

    fn list_tabs(&self, task_id: &str) -> Vec<TabInfo> {
        self.with_state(|state| {
            let Some(task) = state
                .project_store
                .projects
                .iter()
                .flat_map(|p| state.project_store.tasks.get(&p.id).into_iter().flatten())
                .find(|t| t.id == task_id)
            else {
                return Vec::new();
            };
            task.tabs
                .iter()
                .map(|tab| TabInfo {
                    tab_id: tab.id.clone(),
                    provider: tab.provider.map(provider_str),
                    title: tab.title.clone(),
                    session_ref: tab
                        .launch_config
                        .as_ref()
                        .and_then(|lc| lc.session.as_ref().map(|s| s.id.clone())),
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn get_task_status(&self, task_id: &str) -> Option<TaskStatus> {
        self.with_state(|state| {
            let task = state
                .project_store
                .projects
                .iter()
                .flat_map(|p| state.project_store.tasks.get(&p.id).into_iter().flatten())
                .find(|t| t.id == task_id)?;

            if task.tabs.is_empty() {
                return Some(TaskStatus::NoTabs);
            }
            let section = SectionId::from_store_key(&task.section_id)?;
            let any_running = task.tabs.iter().any(|tab| {
                state.broadcasts.contains_key(&TerminalRuntimeKey {
                    section_id: section.clone(),
                    tab_id: tab.id.clone(),
                })
            });
            Some(if any_running {
                TaskStatus::Working
            } else {
                TaskStatus::Idle
            })
        })
        .flatten()
    }

    fn read_terminal_output(&self, tab_id: &str, _tail: usize) -> Option<TerminalSnapshot> {
        // Today `recent_output` lives on the GPUI-side `TerminalManager`
        // and isn't mirrored into `RegistryState`. Returning `None`
        // here is the correct behaviour for the Phase B cut — the
        // follow-up that wires this extends `RegistryState` with
        // a thread-safe snapshot mirror.
        //
        // We can still answer "does this tab exist?" from the
        // project store, which callers commonly want as a
        // precondition check.
        self.with_state(|state| {
            let exists = state
                .project_store
                .projects
                .iter()
                .flat_map(|p| state.project_store.tasks.get(&p.id).into_iter().flatten())
                .any(|t| t.tabs.iter().any(|tab| tab.id == tab_id));
            if exists {
                Some(TerminalSnapshot {
                    bytes: Vec::new(),
                    truncated_head: false,
                })
            } else {
                None
            }
        })
        .flatten()
    }

    // ---- Write tools (#35) ----

    fn spawn_task(&self, _req: SpawnTaskRequest) -> anyhow::Result<SpawnTaskResponse> {
        Err(anyhow::anyhow!(NOT_YET_WIRED))
    }

    fn spawn_terminal(&self, req: SpawnTerminalRequest) -> anyhow::Result<SpawnTerminalResponse> {
        if req.project_id.is_none() && req.task_id.is_none() {
            anyhow::bail!("spawn_terminal requires one of 'project_id' or 'task_id'");
        }
        // Bounded sync channel; capacity 1 because exactly one
        // response message ever travels through it.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        {
            let arc = self
                .inner
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("desktop registry has been dropped"))?;
            let mut state = arc
                .lock()
                .map_err(|_| anyhow::anyhow!("registry mutex poisoned"))?;
            state
                .pending_spawn_terminals
                .push(crate::daemon_host::PendingSpawnTerminal {
                    project_id: req.project_id,
                    task_id: req.task_id,
                    cwd: req.cwd,
                    client_handle: None,
                    responder: tx,
                });
        }
        // Wait for the GPUI render tick to drain. 30 s is generous —
        // the drain runs on every refresh tick (≤ 250 ms idle), so a
        // healthy app responds in well under a second. The cap keeps
        // a wedged GPUI thread from holding the MCP worker forever.
        let resp = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|err| anyhow::anyhow!("spawn_terminal: render-tick drain timed out: {err}"))?;
        resp.map_err(|msg| anyhow::anyhow!(msg))
    }

    fn send_input(&self, tab_id: &str, bytes: &[u8]) -> anyhow::Result<()> {
        // Resolve the writer by tab_id alone — `writers` is keyed by
        // `TerminalRuntimeKey` (section_id + tab_id), but tab ids are
        // UUIDs so a direct scan is unambiguous and works for tabs
        // attached to either a Task or a project-root section. The
        // earlier code walked `project_store.tasks` to recover the
        // section_id, which silently lost project-root tabs (the
        // ones MCP `spawn_terminal` creates).
        let writer = self
            .with_state(|state| {
                state
                    .writers
                    .iter()
                    .find_map(|(key, w)| (key.tab_id == tab_id).then(|| w.clone()))
            })
            .flatten();

        let Some(writer) = writer else {
            anyhow::bail!("tab {tab_id} is not live (no PTY writer)");
        };

        // Same ordering / poison-recovery reasoning as
        // `DesktopTerminalRegistry::tab_input`.
        use std::io::Write;
        let mut guard = match writer.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard
            .write_all(bytes)
            .map_err(|e| anyhow::anyhow!("PTY write failed: {e}"))?;
        guard
            .flush()
            .map_err(|e| anyhow::anyhow!("PTY flush failed: {e}"))?;
        Ok(())
    }

    fn run_command(&self, _req: RunCommandRequest) -> anyhow::Result<RunCommandResponse> {
        Err(anyhow::anyhow!(NOT_YET_WIRED))
    }

    fn subscribe_events(
        &self,
    ) -> Option<tokio::sync::broadcast::Receiver<another_one_core::clients::ClientEvent>> {
        Some(self.event_bus.subscribe())
    }

    fn dispatch_ui_action(
        &self,
        action: another_one_core::mcp::orchestrator::UiAction,
    ) -> anyhow::Result<()> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        {
            let arc = self
                .inner
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("desktop registry has been dropped"))?;
            let mut state = arc
                .lock()
                .map_err(|_| anyhow::anyhow!("registry mutex poisoned"))?;
            state
                .pending_ui_actions
                .push(crate::daemon_host::PendingUiAction {
                    action,
                    responder: tx,
                });
        }
        let resp = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|err| {
                anyhow::anyhow!("dispatch_ui_action: render-tick drain timed out: {err}")
            })?;
        resp.map_err(|msg| anyhow::anyhow!(msg))
    }

    fn select_focus(
        &self,
        req: another_one_core::mcp::orchestrator::SelectFocusRequest,
    ) -> anyhow::Result<()> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        {
            let arc = self
                .inner
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("desktop registry has been dropped"))?;
            let mut state = arc
                .lock()
                .map_err(|_| anyhow::anyhow!("registry mutex poisoned"))?;
            state
                .pending_select_focus
                .push(crate::daemon_host::PendingSelectFocus {
                    focus: req.focus,
                    for_client: req.for_client,
                    client_handle: None,
                    responder: tx,
                });
        }
        let resp = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|err| anyhow::anyhow!("select_focus: render-tick drain timed out: {err}"))?;
        resp.map_err(|msg| anyhow::anyhow!(msg))
    }

    fn close_tab(&self, tab_id: &str) -> anyhow::Result<()> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        {
            let arc = self
                .inner
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("desktop registry has been dropped"))?;
            let mut state = arc
                .lock()
                .map_err(|_| anyhow::anyhow!("registry mutex poisoned"))?;
            state
                .pending_close_tabs
                .push(crate::daemon_host::PendingCloseTab {
                    tab_id: tab_id.to_string(),
                    client_handle: None,
                    responder: tx,
                });
        }
        let resp = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|err| anyhow::anyhow!("close_tab: render-tick drain timed out: {err}"))?;
        resp.map_err(|msg| anyhow::anyhow!(msg))
    }
}

/// Build an orchestrator handle wrapped in the trait-object
/// `Arc` the daemon expects.
pub(crate) fn arc(
    inner: Weak<Mutex<RegistryState>>,
    event_bus: tokio::sync::broadcast::Sender<another_one_core::clients::ClientEvent>,
) -> Arc<dyn McpOrchestrator> {
    Arc::new(DesktopMcpOrchestrator::new(inner, event_bus))
}

fn provider_str(kind: AgentProviderKind) -> String {
    match kind {
        AgentProviderKind::ClaudeCode => "claude-code",
        AgentProviderKind::CursorAgent => "cursor-agent",
        AgentProviderKind::Codex => "codex",
        AgentProviderKind::Pi => "pi",
        AgentProviderKind::Gemini => "gemini",
        AgentProviderKind::OpenCode => "opencode",
        AgentProviderKind::Amp => "amp",
        AgentProviderKind::RovoDev => "rovo-dev",
        AgentProviderKind::Forge => "forge",
    }
    .to_string()
}
