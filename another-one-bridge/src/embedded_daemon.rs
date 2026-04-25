//! Boot the embedded daemon in-process (Flutter desktop only).
//!
//! Replaces the GPUI binary's `desktop::daemon_host` for the future
//! Flutter desktop. The bridge:
//!
//!   1. Loads the on-disk `ProjectStore` and constructs a
//!      [`RegistryState`].
//!   2. Wraps it in a `BridgeTerminalRegistry` that mirrors
//!      `desktop::DesktopTerminalRegistry`'s semantics but reads
//!      from the bridge's own state (no `AnotherOneApp` to project
//!      from).
//!   3. Spawns a dedicated OS thread with a tokio runtime, blocks on
//!      [`daemon_sandbox::run_endpoint`], and on success registers
//!      the resulting [`EndpointHandle`] with both
//!      [`crate::local_pair`] and [`crate::local_registry`] so
//!      `LocalSession` + the pair-mobile FRB API can read from
//!      it.
//!
//! Idempotent at the registration layer: both seams use
//! `OnceLock`, so a second `boot_embedded_daemon` call from Dart is
//! a no-op (we early-out before spawning the thread).
//!
//! MCP transport is not started here — the GPUI desktop owns that
//! today. Phase 6 of the migration will move MCP wiring into core
//! and fold it back in.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;

use tokio::sync::broadcast;

use another_one_core::daemon_embed::{
    daemon_paths, key_from_wire, RegistryState, TabLaunchRequest,
};
use another_one_core::project_store::ProjectStore;
use another_one_core::terminal_types::TerminalRuntimeKey;

use daemon_sandbox::frame::ProjectSummary;
use daemon_sandbox::{EndpointHandle, TerminalRegistry};

use crate::local_pair::{set_local_pair_info, LocalPairInfo};
use crate::local_registry::set_local_registry;

/// Tracks whether the embedded daemon has been booted in this
/// process. `OnceLock` so two concurrent `boot_embedded_daemon`
/// calls from Dart resolve to the same boot.
static BOOTED: OnceLock<()> = OnceLock::new();

/// Build, register, and boot the embedded daemon. Idempotent: a
/// second call no-ops. Returns as soon as the registry is wired and
/// the daemon thread is spawned; the endpoint handshake completes
/// asynchronously on its own runtime, so [`crate::api::pair::pairing_info`]
/// may return `None` for a few hundred milliseconds after this
/// returns. The pair-mobile UI's empty state covers that window.
pub(crate) fn boot() -> Result<(), String> {
    if BOOTED.get().is_some() {
        return Ok(());
    }

    let store = ProjectStore::load();
    let registry_state = Arc::new(Mutex::new(RegistryState::new(store)));
    set_local_registry(registry_state.clone());

    thread::Builder::new()
        .name("another-one-embedded-daemon".into())
        .spawn(move || run(registry_state))
        .map_err(|e| format!("spawn embedded daemon thread: {e}"))?;

    // Mark booted now — even if the endpoint handshake later fails,
    // we don't want a second `boot_embedded_daemon` call to spawn a
    // duplicate registry. The Dart UI surfaces "daemon not ready"
    // for as long as `local_pair_info()` is unset.
    let _ = BOOTED.set(());
    Ok(())
}

fn run(registry_state: Arc<Mutex<RegistryState>>) {
    // Mirrors `desktop::daemon_host::run`. Four workers: a single
    // stuck PTY write under `block_in_place` shouldn't be able to
    // starve the accept loop + writers + forwarders all at once.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .thread_name("another-one-embedded-rt")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("build embedded daemon runtime: {e:#}");
            return;
        }
    };

    let weak = Arc::downgrade(&registry_state);
    drop(registry_state);
    let registry: Arc<dyn TerminalRegistry> = Arc::new(BridgeTerminalRegistry::new(weak));

    let paths = match daemon_paths() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("resolve daemon paths: {e:#}");
            return;
        }
    };

    let endpoint_result = runtime.block_on(async {
        daemon_sandbox::run_endpoint(registry, paths.secret_key, paths.paired_peers).await
    });

    match endpoint_result {
        Ok(handle) => {
            let adapter: Arc<dyn LocalPairInfo> =
                Arc::new(EndpointHandlePairAdapter::new(Arc::new(handle)));
            set_local_pair_info(adapter);
            // Park forever — the handle stays in `set_local_pair_info`'s
            // `OnceLock` for the rest of the process. Dropping the
            // runtime would tear down the endpoint.
            loop {
                thread::park();
            }
        }
        Err(e) => {
            tracing::error!("embedded daemon boot failed: {e:#}");
        }
    }
}

/// Adapts an `EndpointHandle` to `LocalPairInfo`. Splitting the
/// trait off the concrete handle is the seam that lets the bridge
/// expose pair info without leaking `daemon_sandbox` types into
/// `crate::local_pair` (which `api/pair.rs` depends on).
struct EndpointHandlePairAdapter {
    handle: Arc<EndpointHandle>,
}

impl EndpointHandlePairAdapter {
    fn new(handle: Arc<EndpointHandle>) -> Self {
        Self { handle }
    }
}

impl LocalPairInfo for EndpointHandlePairAdapter {
    fn pairing_url(&self) -> String {
        self.handle.pairing_url()
    }

    fn qr_png_bytes(&self) -> Vec<u8> {
        self.handle.qr_png_bytes()
    }

    fn regenerate_pairing(&self) -> Result<(), String> {
        self.handle
            .regenerate_pairing()
            .map_err(|e| format!("{e:#}"))
    }
}

/// `TerminalRegistry` impl that operates directly on the bridge's
/// `RegistryState`. Mirrors `desktop::DesktopTerminalRegistry` but
/// without the desktop's project-summary projection logic — that
/// will return when the Flutter desktop port owns the project tree
/// directly. For now `list_projects` flattens the in-memory store
/// the same way `LocalSession::list_projects` does (see
/// `api/local_session.rs::flatten_project_store`).
struct BridgeTerminalRegistry {
    inner: Weak<Mutex<RegistryState>>,
}

impl BridgeTerminalRegistry {
    fn new(inner: Weak<Mutex<RegistryState>>) -> Self {
        Self { inner }
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut RegistryState) -> R) -> Option<R> {
        let arc = self.inner.upgrade()?;
        let mut guard = arc.lock().ok()?;
        Some(f(&mut guard))
    }
}

impl TerminalRegistry for BridgeTerminalRegistry {
    fn list_projects(&self) -> Vec<ProjectSummary> {
        // Iroh peers list nothing until the Flutter desktop owns the
        // project tree itself. Returning `vec![]` is a real "no
        // projects yet" state, not an error.
        Vec::new()
    }

    fn attach_tab(
        &self,
        section_id: &str,
        tab_id: &str,
    ) -> Option<broadcast::Receiver<Vec<u8>>> {
        let key = key_from_wire(section_id, tab_id)?;
        self.with_state(|state| state.broadcasts.get(&key).map(|tx| tx.subscribe()))
            .flatten()
    }

    fn tab_input(&self, section_id: &str, tab_id: &str, bytes: &[u8]) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        // Clone writer Arc out, drop state lock, then write — same
        // rationale as desktop::DesktopTerminalRegistry::tab_input.
        let writer = self
            .with_state(|state| state.writers.get(&key).cloned())
            .flatten();
        let Some(writer) = writer else { return };
        tokio::task::block_in_place(|| {
            let mut guard = match writer.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let _ = guard.write_all(bytes);
            let _ = guard.flush();
        });
    }

    fn tab_resize(
        &self,
        viewer_id: &str,
        section_id: &str,
        tab_id: &str,
        cols: u16,
        rows: u16,
    ) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        self.with_state(|state| {
            if let Some(old_key) = state.viewer_focus.get(viewer_id).cloned() {
                if old_key != key {
                    if let Some(map) = state.active_viewers.get_mut(&old_key) {
                        map.remove(viewer_id);
                        if map.is_empty() {
                            state.active_viewers.remove(&old_key);
                            state.effective_sizes.remove(&old_key);
                        }
                    }
                    state.recompute_effective_size(&old_key);
                }
            }
            state
                .active_viewers
                .entry(key.clone())
                .or_default()
                .insert(viewer_id.to_string(), (cols, rows));
            state
                .viewer_focus
                .insert(viewer_id.to_string(), key.clone());
            state.recompute_effective_size(&key);
        });
    }

    fn viewer_disconnected(&self, viewer_id: &str) {
        self.with_state(|state| {
            state.viewer_focus.remove(viewer_id);
            let touched: Vec<TerminalRuntimeKey> = state
                .active_viewers
                .iter_mut()
                .filter_map(|(key, map)| {
                    if map.remove(viewer_id).is_some() {
                        Some(key.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for key in touched {
                let empty = state
                    .active_viewers
                    .get(&key)
                    .map(|m| m.is_empty())
                    .unwrap_or(true);
                if empty {
                    state.active_viewers.remove(&key);
                    state.effective_sizes.remove(&key);
                } else {
                    state.recompute_effective_size(&key);
                }
            }
        });
    }

    fn launch_tab(&self, section_id: &str, tab_id: &str) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        self.with_state(|state| {
            if state.broadcasts.contains_key(&key) {
                return;
            }
            if state.in_flight_launches.contains(&key) {
                return;
            }
            if state.pending_tab_launches.iter().any(|r| r.key == key) {
                return;
            }
            state.pending_tab_launches.push(TabLaunchRequest { key });
        });
    }
}
