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
//!        * `Output` → append to the tab's bounded replay buffer and
//!          forward to any live `attach_tab` subscribers.
//!        * `SessionDiscovered` → no-op (session restoration is
//!          GPUI-only today).
//!        * `Exited` / `Failed` → drop the live entry + remove
//!          the registry's `broadcasts` / `writers` entries.
//!   3. Drains [`RegistryState::pending_resizes`] and forwards
//!      each request to the matching live PTY's master handle.
//!   4. Drains [`RegistryState::pending_tab_terminations`] and
//!      tears down any matching live PTY after section/tab
//!      mutators remove it from the store.
//!
//! What this drain DOESN'T do (vs GPUI's tick): no VT-parsing,
//! no snapshot rebuilds, no toast surfacing. Flutter's
//! `xterm.dart` consumes the raw byte stream directly; launch
//! failures are persisted on the tab and rendered from the project
//! summary state.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

use another_one_core::agents::{
    agent_id_for_provider, AgentProviderKind, TerminalLaunchConfig, TerminalRestoreStatus, AGENTS,
};
use another_one_core::daemon_embed::{
    RegistryState, TabLaunchRequest, TabResizeRequest, TerminalReplayBuffer,
};
use another_one_core::process::TrackedProcess;
use another_one_core::terminal_launch::{spawn_terminal_launch, TerminalLaunchReply};
use another_one_core::terminal_types::{
    PreparedTerminalRuntime, TerminalGridSize, TerminalRuntimeKey,
};

const DESKTOP_BASELINE_COLS: u16 = 100;
const DESKTOP_BASELINE_ROWS: u16 = 30;

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
                Ok(TerminalLaunchReply::Output { key, bytes }) => {
                    handle_output(&registry, &key, bytes);
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
                Ok(TerminalLaunchReply::Exited { key, .. }) => {
                    handle_terminated(&registry, &mut live, &key);
                }
                Ok(TerminalLaunchReply::Failed {
                    key,
                    message,
                    details,
                }) => {
                    handle_failed_launch(&registry, &mut live, &key, message, details);
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
        let pending_resizes = {
            let mut state = match registry.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            std::mem::take(&mut state.pending_resizes)
                .into_iter()
                .filter_map(|request| {
                    resize_request_for_effective_size(
                        &request.key,
                        state.effective_sizes.get(&request.key).copied(),
                    )
                })
                .collect::<Vec<_>>()
        };
        for request in pending_resizes {
            apply_resize_request(&mut live, &request);
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
    let launch_size = launch_size_for_effective_size(state.effective_sizes.get(&req.key).copied());
    state.in_flight_launches.insert(req.key.clone());
    let section_key = req.key.section_id.store_key();
    state.project_store.set_tab_restore_status(
        &section_key,
        &req.key.tab_id,
        TerminalRestoreStatus::Launching,
        None,
        None,
    );
    drop(state);
    spawn_terminal_launch(
        tx,
        req.key,
        Some(cwd),
        launch_config,
        agent_args,
        launch_size,
    );
}

fn launch_size_for_effective_size(effective_size: Option<(u16, u16)>) -> TerminalGridSize {
    let (cols, rows) = effective_size.unwrap_or((DESKTOP_BASELINE_COLS, DESKTOP_BASELINE_ROWS));
    TerminalGridSize {
        cols: cols.max(1),
        rows: rows.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn resize_request_for_effective_size(
    key: &TerminalRuntimeKey,
    effective_size: Option<(u16, u16)>,
) -> Option<TabResizeRequest> {
    let (cols, rows) = effective_size?;
    Some(TabResizeRequest {
        key: key.clone(),
        cols: cols.max(1),
        rows: rows.max(1),
    })
}

fn effective_resize_request(
    key: &TerminalRuntimeKey,
    current_size: TerminalGridSize,
    effective_size: Option<(u16, u16)>,
) -> Option<TabResizeRequest> {
    let request = resize_request_for_effective_size(key, effective_size)?;
    if current_size.cols == request.cols && current_size.rows == request.rows {
        return None;
    }
    Some(request)
}

fn apply_resize_request_to_size<F>(
    size: &mut TerminalGridSize,
    request: &TabResizeRequest,
    mut resize: F,
) -> bool
where
    F: FnMut(TerminalGridSize) -> bool,
{
    if size.cols == request.cols && size.rows == request.rows {
        return true;
    }
    let mut next_size = *size;
    next_size.cols = request.cols.max(1);
    next_size.rows = request.rows.max(1);
    if !resize(next_size) {
        return false;
    }
    *size = next_size;
    true
}

fn apply_resize_request(
    live: &mut HashMap<TerminalRuntimeKey, LiveTab>,
    request: &TabResizeRequest,
) -> bool {
    let Some(tab) = live.get_mut(&request.key) else {
        return false;
    };
    let runtime = &mut tab._runtime_handle;
    let master = &runtime.master;
    apply_resize_request_to_size(&mut runtime.size, request, |size| {
        master.resize(size.as_pty_size()).is_ok()
    })
}

fn handle_output(registry: &Arc<Mutex<RegistryState>>, key: &TerminalRuntimeKey, bytes: Vec<u8>) {
    let mut state = match registry.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    let Some(sender) = state.broadcasts.get(key).cloned() else {
        return;
    };
    if sender.receiver_count() == 0 {
        state
            .terminal_replay
            .entry(key.clone())
            .or_default()
            .push(bytes);
        return;
    }
    state
        .terminal_replay
        .entry(key.clone())
        .or_default()
        .push(bytes.clone());
    let _ = sender.send(bytes);
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
            clear_viewer_output_cursors(&mut state, &key);
        }
        return;
    }

    // Take writer + broadcast out of the runtime struct without
    // dropping `master` / `child_killer` (which would SIGHUP the
    // child). The broadcast Sender is `Clone`; the writer is a
    // trait object so we swap a no-op sink in its place.
    let launch_size = runtime.size;
    let writer = std::mem::replace(&mut runtime.writer, Box::new(std::io::sink()));
    let writer_arc: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(writer));
    let output_broadcast = runtime.output_broadcast.clone();
    live.insert(
        key.clone(),
        LiveTab {
            _runtime_handle: runtime,
        },
    );
    let (pending_input, pending_resize) = {
        let mut state = match registry.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        state.broadcasts.insert(key.clone(), output_broadcast);
        state
            .terminal_replay
            .insert(key.clone(), TerminalReplayBuffer::default());
        clear_viewer_output_cursors(&mut state, &key);
        state.writers.insert(key.clone(), writer_arc.clone());
        state.in_flight_launches.remove(&key);
        let section_key = key.section_id.store_key();
        state.project_store.set_tab_restore_status(
            &section_key,
            &key.tab_id,
            TerminalRestoreStatus::Ready,
            None,
            None,
        );
        if let Some(pid) = process_id {
            let tracked = build_tracked_process(&state, &key, &launch_config, pid);
            state.tracked_processes.insert(key.clone(), tracked);
        }
        (
            state.pending_post_launch_input.remove(&key),
            effective_resize_request(&key, launch_size, state.effective_sizes.get(&key).copied()),
        )
    };
    if let Some(request) = pending_resize {
        apply_resize_request(live, &request);
    }
    if let Some(bytes) = pending_input {
        let result = writer_arc
            .lock()
            .map_err(|_| "terminal writer mutex poisoned".to_string())
            .and_then(|mut writer| {
                writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .map_err(|e| e.to_string())
            });
        if let Err(details) = result {
            handle_writer_failed(
                registry,
                live,
                &key,
                "Terminal input failed during post-launch replay".to_string(),
                details,
            );
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
    state.terminal_replay.remove(key);
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
    clear_viewer_output_cursors(&mut state, key);
    state
        .viewer_focus
        .retain(|_, focused_key| focused_key != key);
}

fn handle_failed_launch(
    registry: &Arc<Mutex<RegistryState>>,
    live: &mut HashMap<TerminalRuntimeKey, LiveTab>,
    key: &TerminalRuntimeKey,
    message: String,
    details: String,
) {
    let section_key = key.section_id.store_key();
    tracing::warn!(
        section_id = %section_key,
        tab_id = %key.tab_id,
        message = message.as_str(),
        details = details.as_str(),
        "terminal launch failed"
    );
    handle_terminated(registry, live, key);
    let mut state = match registry.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    state.project_store.set_tab_restore_status(
        &section_key,
        &key.tab_id,
        TerminalRestoreStatus::Failed,
        Some(message),
        Some(details),
    );
}

fn handle_writer_failed(
    registry: &Arc<Mutex<RegistryState>>,
    live: &mut HashMap<TerminalRuntimeKey, LiveTab>,
    key: &TerminalRuntimeKey,
    message: String,
    details: String,
) {
    tracing::warn!(
        section_id = %key.section_id.store_key(),
        tab_id = %key.tab_id,
        message = message.as_str(),
        details = details.as_str(),
        "terminal writer failed"
    );
    live.remove(key);
    let mut state = match registry.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    state.fail_tab_io(key, message, details);
}

fn clear_viewer_output_cursors(state: &mut RegistryState, key: &TerminalRuntimeKey) {
    state
        .viewer_output_cursors
        .retain(|(_, cursor_key), _| cursor_key != key);
}

#[cfg(test)]
mod tests {
    use super::{
        apply_resize_request_to_size, effective_resize_request, launch_size_for_effective_size,
        resize_request_for_effective_size, DESKTOP_BASELINE_COLS, DESKTOP_BASELINE_ROWS,
    };
    use another_one_core::daemon_embed::TabResizeRequest;
    use another_one_core::section::SectionId;
    use another_one_core::terminal_types::{TerminalGridSize, TerminalRuntimeKey};

    fn test_key() -> TerminalRuntimeKey {
        TerminalRuntimeKey {
            section_id: SectionId::new("project-1", "main"),
            tab_id: "tab-1".to_string(),
        }
    }

    #[test]
    fn apply_resize_request_to_size_updates_grid_and_preserves_pixels() {
        let key = test_key();
        let request = TabResizeRequest {
            key,
            cols: 120,
            rows: 40,
        };
        let mut size = TerminalGridSize {
            cols: 80,
            rows: 24,
            pixel_width: 17,
            pixel_height: 33,
        };
        let mut resized_to = Vec::new();

        let applied = apply_resize_request_to_size(&mut size, &request, |next_size| {
            resized_to.push(next_size);
            true
        });

        assert!(applied);
        assert_eq!(
            size,
            TerminalGridSize {
                cols: 120,
                rows: 40,
                pixel_width: 17,
                pixel_height: 33,
            }
        );
        assert_eq!(
            resized_to,
            vec![TerminalGridSize {
                cols: 120,
                rows: 40,
                pixel_width: 17,
                pixel_height: 33,
            }]
        );
    }

    #[test]
    fn apply_resize_request_to_size_skips_redundant_resize() {
        let key = test_key();
        let request = TabResizeRequest {
            key,
            cols: 80,
            rows: 24,
        };
        let mut size = TerminalGridSize {
            cols: 80,
            rows: 24,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut called = false;

        let applied = apply_resize_request_to_size(&mut size, &request, |_next_size| {
            called = true;
            true
        });

        assert!(applied);
        assert!(!called);
        assert_eq!(
            size,
            TerminalGridSize {
                cols: 80,
                rows: 24,
                pixel_width: 0,
                pixel_height: 0,
            }
        );
    }

    #[test]
    fn apply_resize_request_to_size_keeps_current_size_when_resize_fails() {
        let key = test_key();
        let request = TabResizeRequest {
            key,
            cols: 132,
            rows: 50,
        };
        let original = TerminalGridSize {
            cols: 80,
            rows: 24,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut size = original;

        let applied = apply_resize_request_to_size(&mut size, &request, |_next_size| false);

        assert!(!applied);
        assert_eq!(size, original);
    }

    #[test]
    fn effective_resize_request_only_returns_when_launch_size_differs() {
        let key = test_key();
        let launch_size = TerminalGridSize {
            cols: 80,
            rows: 24,
            pixel_width: 0,
            pixel_height: 0,
        };

        assert!(effective_resize_request(&key, launch_size, None).is_none());
        assert!(effective_resize_request(&key, launch_size, Some((80, 24))).is_none());

        let request = effective_resize_request(&key, launch_size, Some((100, 30)))
            .expect("changed effective size should produce a resize request");
        assert_eq!(request.key, key);
        assert_eq!(request.cols, 100);
        assert_eq!(request.rows, 30);
    }

    #[test]
    fn launch_size_for_effective_size_uses_viewer_size_when_available() {
        assert_eq!(
            launch_size_for_effective_size(Some((132, 50))),
            TerminalGridSize {
                cols: 132,
                rows: 50,
                pixel_width: 0,
                pixel_height: 0,
            }
        );
    }

    #[test]
    fn launch_size_for_effective_size_defaults_to_desktop_baseline_size() {
        assert_eq!(
            launch_size_for_effective_size(None),
            TerminalGridSize {
                cols: DESKTOP_BASELINE_COLS,
                rows: DESKTOP_BASELINE_ROWS,
                pixel_width: 0,
                pixel_height: 0,
            }
        );
    }

    #[test]
    fn resize_request_for_effective_size_returns_none_when_viewer_state_is_gone() {
        let key = test_key();

        assert!(resize_request_for_effective_size(&key, None).is_none());

        let request = resize_request_for_effective_size(&key, Some((0, 0)))
            .expect("effective sizes should clamp to a non-zero resize request");
        assert_eq!(request.key, key);
        assert_eq!(request.cols, 1);
        assert_eq!(request.rows, 1);
    }
}
