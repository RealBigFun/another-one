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
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

use another_one_core::agents::{
    agent_id_for_provider, TerminalLaunchConfig,
};
use another_one_core::daemon_embed::{RegistryState, TabLaunchRequest};
use another_one_core::terminal_launch::{
    spawn_terminal_launch, TerminalLaunchReply,
};
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
                    key, runtime, ..
                }) => {
                    handle_launched(&registry, &mut live, key, runtime);
                }
                Ok(TerminalLaunchReply::Output { .. }) => {
                    // The PTY reader already broadcasts each chunk
                    // via `output_broadcast` (we route that to
                    // attached viewers in `attach_tab`); no extra
                    // bookkeeping needed here.
                }
                Ok(TerminalLaunchReply::SessionDiscovered { .. }) => {
                    // Session restoration is desktop-side only.
                }
                Ok(TerminalLaunchReply::Exited { key, .. })
                | Ok(TerminalLaunchReply::Failed { key, .. }) => {
                    handle_terminated(&registry, &mut live, &key);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
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
    let Some((cwd, launch_config, agent_args)) =
        resolve_launch_inputs(&state, &req.key)
    else {
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
    let tab = section?
        .tabs
        .iter()
        .find(|t| t.id == key.tab_id)?;
    let launch_config = tab
        .launch_config
        .clone()
        .unwrap_or_default();

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
) {
    // Take writer + broadcast out of the runtime struct without
    // dropping `master` / `child_killer` (which would SIGHUP the
    // child). The broadcast Sender is `Clone`; the writer is a
    // trait object so we swap a no-op sink in its place.
    let writer = std::mem::replace(
        &mut runtime.writer,
        Box::new(std::io::sink()),
    );
    let writer_arc: Arc<Mutex<Box<dyn Write + Send>>> =
        Arc::new(Mutex::new(writer));
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
        state.pending_post_launch_input.remove(&key)
    };
    if let Some(bytes) = pending_input {
        if let Ok(mut writer) = writer_arc.lock() {
            let _ = writer.write_all(&bytes);
            let _ = writer.flush();
        }
    }
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
    state.pending_post_launch_input.remove(key);
}
