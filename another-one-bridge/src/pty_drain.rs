//! Drain `RegistryState::pending_tab_launches` into live PTYs.
//!
//! The Flutter desktop runs the bridge's embedded daemon
//! (`embedded_daemon::run`); GPUI's render-tick drain doesn't
//! exist here. This module ports the slimmed-down equivalent: a
//! dedicated OS thread that polls the registry every 50ms and
//!
//!   1. Pulls every queued [`TabLaunchRequest`] out of the
//!      registry, resolves cwd + launch config + agent launch
//!      args from the project store, and calls
//!      [`spawn_terminal_launch`] for each.
//!   2. Drains the reply mpsc and routes:
//!        * `Launched` → publish `output_broadcast` + `writer`
//!          on the registry so [`LocalSession::attach_tab`] +
//!          [`LocalSession::send`] resolve. Hold on to the
//!          `master` + `child_killer` locally so the PTY's child
//!          doesn't get SIGHUP. Drain
//!          `pending_post_launch_input` if recorded.
//!        * `Output` → no-op. The PTY reader's
//!          [`tokio::sync::broadcast`] tee already routes to
//!          subscribers via `attach_tab`'s forwarder loop.
//!        * `SessionDiscovered` → no-op (session restoration is
//!          GPUI-only today).
//!        * `Exited` / `Failed` → drop the live entry + remove
//!          the registry's `broadcasts` / `writers` entries.
//!
//! What this drain DOESN'T do (vs GPUI's tick): no VT-parsing,
//! no snapshot rebuilds, no toast surfacing. Flutter's
//! `xterm.dart` consumes the raw byte stream directly, and
//! launch-failure surfacing is the next iteration's work.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::Duration;

use another_one_core::agents::{
    AGENTS, AgentProviderKind, TerminalLaunchConfig, agent_id_for_provider,
};
use another_one_core::daemon_embed::{RegistryState, TabLaunchRequest};
use another_one_core::process::TrackedProcess;
use another_one_core::terminal_launch::{TerminalLaunchReply, spawn_terminal_launch};
use another_one_core::terminal_types::{
    PreparedTerminalRuntime, TerminalGridSize, TerminalRuntimeKey,
};

/// Per-key local state pinning the PTY alive. Holding the full
/// [`PreparedTerminalRuntime`] (minus the writer + broadcast,
/// which we hand off to the registry) is the simplest way to
/// keep `master` and `child_killer` alive without re-exporting
/// the underlying portable-pty trait objects from core.
///
/// Dropping this closes the master fd and SIGHUPs the child —
/// only drop on `Exited` / `Failed`.
struct LiveTab {
    /// `runtime.writer` and `runtime.output_broadcast` are
    /// `take`n in `handle_launched`; the remaining fields keep
    /// the PTY alive.
    _runtime_handle: PreparedTerminalRuntime,
}

/// Spawn the drain thread. Returns immediately. The thread runs
/// for the lifetime of the process and exits cleanly when the
/// `Weak<RegistryState>` upgrades to `None` (the daemon's
/// shutdown signal).
pub fn spawn_drain(registry: Arc<Mutex<RegistryState>>) {
    let weak = Arc::downgrade(&registry);
    thread::Builder::new()
        .name("another-one-pty-drain".into())
        .spawn(move || run(weak))
        .expect("spawn pty-drain thread");
}

fn run(weak: Weak<Mutex<RegistryState>>) {
    let (tx, rx) = mpsc::sync_channel::<TerminalLaunchReply>(1024);
    let mut live: HashMap<TerminalRuntimeKey, LiveTab> = HashMap::new();
    loop {
        let Some(registry) = weak.upgrade() else {
            return;
        };
        // Pull every queued launch out of the registry under one
        // lock. Each iteration drains the whole batch; the spawn
        // calls below run lock-free.
        let pending = {
            let mut state = match registry.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            std::mem::take(&mut state.pending_tab_launches)
        };
        for req in pending {
            spawn_one(&registry, &mut live, req, tx.clone());
        }
        // Drain replies — bounded by the channel itself so a
        // burst doesn't starve the next launch tick.
        loop {
            match rx.try_recv() {
                Ok(TerminalLaunchReply::Launched {
                    key,
                    runtime,
                    launch_config,
                    process_id,
                }) => {
                    handle_launched(
                        &registry,
                        &mut live,
                        key,
                        runtime,
                        launch_config,
                        process_id,
                    );
                }
                Ok(TerminalLaunchReply::Output { .. }) => {
                    // The PTY reader already broadcasts each chunk
                    // via `output_broadcast` (we route that to
                    // attached viewers in `attach_tab`); no extra
                    // bookkeeping needed here.
                }
                Ok(TerminalLaunchReply::SessionDiscovered { key, session }) => {
                    // GPUI's `AnotherOneApp` used to write the
                    // discovered Claude / Codex session id back into
                    // the persisted tab's launch_config; the bridge
                    // owns that responsibility now. Without this,
                    // every app restart mints a fresh session uuid
                    // and the agent starts a brand-new conversation.
                    if let Ok(mut state) = registry.lock() {
                        let section_key = key.section_id.store_key();
                        state
                            .project_store
                            .set_tab_session(&section_key, &key.tab_id, session);
                    }
                }
                Ok(TerminalLaunchReply::Exited { key, .. })
                | Ok(TerminalLaunchReply::Failed { key, .. }) => {
                    handle_terminated(&registry, &mut live, &key);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        let pending_terminations = {
            let mut state = match registry.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            std::mem::take(&mut state.pending_tab_terminations)
        };
        for key in pending_terminations {
            handle_terminated(&registry, &mut live, &key);
        }
        drop(registry);
        thread::sleep(Duration::from_millis(50));
    }
}

/// Resolve cwd + launch config + agent args for a queued launch
/// and call [`spawn_terminal_launch`]. Inserts the key into
/// `in_flight_launches` so a concurrent `LaunchTab` for the same
/// key is deduped on the registry side.
fn spawn_one(
    registry: &Arc<Mutex<RegistryState>>,
    live: &mut HashMap<TerminalRuntimeKey, LiveTab>,
    req: TabLaunchRequest,
    tx: mpsc::SyncSender<TerminalLaunchReply>,
) {
    if live.contains_key(&req.key) {
        return;
    }
    let mut state = match registry.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    if state.broadcasts.contains_key(&req.key) {
        return;
    }
    let Some((cwd, launch_config, agent_args)) = resolve_launch_inputs(&state, &req.key) else {
        return;
    };
    state.in_flight_launches.insert(req.key.clone());
    drop(state);
    let size = TerminalGridSize {
        cols: 80,
        rows: 24,
        pixel_width: 0,
        pixel_height: 0,
    };
    spawn_terminal_launch(tx, req.key, Some(cwd), launch_config, agent_args, size);
}

fn resolve_launch_inputs(
    state: &RegistryState,
    key: &TerminalRuntimeKey,
) -> Option<(PathBuf, TerminalLaunchConfig, Vec<String>)> {
    let store = &state.project_store;
    // Resolve cwd: fall back to the section's project path when
    // the persisted SectionState doesn't carry an override.
    let section_key = key.section_id.store_key();
    let section = store.terminal_sections.get(&section_key);
    let project = store.project(&key.section_id.project_id)?;
    let cwd = section
        .and_then(|s| s.cwd.clone())
        .unwrap_or_else(|| project.path.clone());

    // Resolve launch config from the persisted tab; missing tab
    // means the queue raced with a tab close — skip the launch.
    let tab = section?.tabs.iter().find(|t| t.id == key.tab_id)?;
    let launch_config = tab.launch_config.clone().unwrap_or_default();

    let agent_args = if launch_config.use_agent_launch_args {
        launch_config
            .provider
            .and_then(agent_id_for_provider)
            .map(|agent_id| store.agent_launch_args(agent_id).to_vec())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Some((cwd, launch_config, agent_args))
}

/// PTY just spawned successfully — publish its broadcast +
/// writer on the registry so `attach_tab` can subscribe and
/// `send` can write keystrokes. Drain `pending_post_launch_input`
/// if the launch came from a custom-action shell run.
fn handle_launched(
    registry: &Arc<Mutex<RegistryState>>,
    live: &mut HashMap<TerminalRuntimeKey, LiveTab>,
    key: TerminalRuntimeKey,
    mut runtime: PreparedTerminalRuntime,
    launch_config: TerminalLaunchConfig,
    process_id: Option<u32>,
) {
    let tab_still_exists = {
        let state = match registry.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        let section_key = key.section_id.store_key();
        state
            .project_store
            .terminal_sections
            .get(&section_key)
            .is_some_and(|section| section.tabs.iter().any(|tab| tab.id == key.tab_id))
    };
    if !tab_still_exists {
        if let Ok(mut state) = registry.lock() {
            state.in_flight_launches.remove(&key);
            state.pending_post_launch_input.remove(&key);
            state.tracked_processes.remove(&key);
        }
        return;
    }

    // Take writer + broadcast out of the runtime struct without
    // dropping `master` / `child_killer` (which would SIGHUP the
    // child). The broadcast Sender is `Clone`; the writer is a
    // trait object so we swap a no-op sink in its place.
    let writer = std::mem::replace(&mut runtime.writer, Box::new(std::io::sink()));
    let writer_arc: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(writer));
    let output_broadcast = runtime.output_broadcast.clone();
    live.insert(
        key.clone(),
        LiveTab {
            _runtime_handle: runtime,
        },
    );
    let pending_input = {
        let mut state = match registry.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        state.broadcasts.insert(key.clone(), output_broadcast);
        state.writers.insert(key.clone(), writer_arc.clone());
        state.in_flight_launches.remove(&key);
        if let Some(pid) = process_id {
            let tracked = build_tracked_process(&state, &key, &launch_config, pid);
            state.tracked_processes.insert(key.clone(), tracked);
        }
        state.pending_post_launch_input.remove(&key)
    };
    if let Some(bytes) = pending_input {
        if let Ok(mut writer) = writer_arc.lock() {
            let _ = writer.write_all(&bytes);
            let _ = writer.flush();
        }
    }
}

/// Build the per-tab `TrackedProcess` row the resource sampler
/// groups by. Mirrors `desktop::AnotherOneApp::tracked_process_for_tab`
/// + `resource_group_for_key` so the popover's project / task /
/// session labels match GPUI's exactly.
fn build_tracked_process(
    state: &RegistryState,
    key: &TerminalRuntimeKey,
    launch_config: &TerminalLaunchConfig,
    pid: u32,
) -> TrackedProcess {
    let (project_key, project_label, task_key, task_label) = resource_group_for_key(state, key);
    TrackedProcess {
        pid,
        key: format!("session:{}:{}", key.section_id.store_key(), key.tab_id),
        label: launch_config.default_title(),
        project_key,
        project_label,
        task_key,
        task_label,
        icon_path: resource_session_icon_path(launch_config.provider),
    }
}

fn resource_group_for_key(
    state: &RegistryState,
    key: &TerminalRuntimeKey,
) -> (String, String, String, String) {
    let store = &state.project_store;
    if let Some(task_id) = key.section_id.task_id.as_deref() {
        if let Some(task) = store.task(task_id) {
            let project_id = task.root_project_id.clone();
            let project_label = store
                .project(&project_id)
                .map(|project| project.name.clone())
                .unwrap_or_else(|| project_id.clone());
            let task_label = if task.name.trim().is_empty() {
                task.branch_name.clone()
            } else {
                task.name.clone()
            };
            return (
                format!("resource-project:{project_id}"),
                project_label,
                format!("resource-task:{}", task.id),
                task_label,
            );
        }
    }
    let project_id = key.section_id.project_id.clone();
    let project = store.project(&project_id);
    let project_label = project
        .map(|project| project.name.clone())
        .unwrap_or_else(|| project_id.clone());
    let task_label = project
        .and_then(|project| project.worktree_name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| key.section_id.branch_name.clone());
    (
        format!("resource-project:{project_id}"),
        project_label,
        format!("resource-task:{}", key.section_id.store_key()),
        task_label,
    )
}

fn resource_session_icon_path(provider: Option<AgentProviderKind>) -> &'static str {
    provider
        .and_then(|provider| {
            AGENTS
                .iter()
                .find(|agent| agent.provider == Some(provider))
                .map(|agent| agent.icon)
        })
        .unwrap_or("assets/icons/icons__terminal.svg")
}

/// Tab's PTY exited or failed to launch. Drop the live entry so
/// the master fd closes, and remove the broadcast + writer
/// entries on the registry so a re-launch starts clean.
fn handle_terminated(
    registry: &Arc<Mutex<RegistryState>>,
    live: &mut HashMap<TerminalRuntimeKey, LiveTab>,
    key: &TerminalRuntimeKey,
) {
    live.remove(key);
    let mut state = match registry.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    state.broadcasts.remove(key);
    state.writers.remove(key);
    state.in_flight_launches.remove(key);
    state
        .pending_tab_launches
        .retain(|request| request.key != *key);
    state.pending_resizes.retain(|request| request.key != *key);
    state.pending_post_launch_input.remove(key);
    state.tracked_processes.remove(key);
    state.active_viewers.remove(key);
    state.effective_sizes.remove(key);
    state
        .viewer_focus
        .retain(|_, focused_key| focused_key != key);
}
