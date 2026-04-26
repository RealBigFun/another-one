//! Trait the embedding process implements so the daemon library can
//! resolve projects/tasks/tabs and attach to live PTYs without
//! reaching into the desktop app's internals. Used by both the
//! standalone sandbox binary (where a minimal impl fakes a single
//! task + tab mapped to a throwaway shell) and by the desktop crate
//! (where the impl wraps `AnotherOneApp`'s real terminal runtimes).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use iroh::EndpointAddr;
use tokio::sync::broadcast;

use crate::frame::ProjectSummary;

/// Boxed-future return type for async methods on [`DaemonRegistry`].
///
/// `DaemonRegistry` is a trait object (callers hold
/// `Arc<dyn DaemonRegistry>`), so methods can't be `async fn` directly
/// — that desugars to a per-impl `impl Future`, which isn't object-safe
/// before the dyn-async-fn-in-trait stabilisation lands and we'd need
/// stricter MSRV bumps anyway. Instead, async methods return a
/// `RegistryFuture<'_, T>` and the per-impl body wraps its work in
/// `Box::pin(async move { … })`.
///
/// Used today by [`DaemonRegistry::add_project`]; future async verbs
/// (e.g. `ojm.5`'s git mutators) reuse this alias rather than
/// re-typing the boxed-future shape per method.
pub type RegistryFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Shared pairing state: the one-shot TOFU nonce the daemon expects
/// in the first `Control::Hello` from any new peer, plus the current
/// pairing URL + QR PNG that encode it. Lives behind an `Arc<Mutex>`
/// so the authorisation path can read (and consume) the nonce while
/// the UI layer reads the URL/QR by snapshot.
///
/// `nonce == None` means "no outstanding pair slot" — either the
/// nonce was already consumed or the daemon hasn't rolled one yet.
/// An unknown peer that arrives in that state is rejected.
pub(crate) struct PairState {
    pub nonce: Option<String>,
    pub addr: EndpointAddr,
    pub pairing_url: String,
    pub qr_png_bytes: Vec<u8>,
}

/// A handle the embedder holds to keep the iroh endpoint alive. Drop
/// to shut down. Exposes the pairing material so the desktop's
/// "Pair mobile" modal can render a live QR without touching /tmp.
pub struct EndpointHandle {
    pub endpoint_id: String,
    pub(crate) pair_state: Arc<Mutex<PairState>>,
    /// Dropped when the handle drops; aborts the endpoint's root
    /// task and all per-connection tasks it spawned.
    pub(crate) _root_task: tokio::task::AbortHandle,
}

impl EndpointHandle {
    /// Snapshot of the currently-published pairing URL. Changes after
    /// [`Self::regenerate_pairing`].
    pub fn pairing_url(&self) -> String {
        self.pair_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pairing_url
            .clone()
    }

    /// Snapshot of the currently-published pairing QR PNG bytes.
    /// Changes after [`Self::regenerate_pairing`].
    pub fn qr_png_bytes(&self) -> Vec<u8> {
        self.pair_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .qr_png_bytes
            .clone()
    }

    /// Roll a fresh TOFU nonce and rebuild the pairing URL + QR. Call
    /// this after the user clicks "Reset pairings" so the previously
    /// scanned QR can no longer pair (even if the attacker captured
    /// it). Cheap — no new endpoint / socket work.
    pub fn regenerate_pairing(&self) -> anyhow::Result<()> {
        // Recover from a poisoned mutex: if a prior regeneration
        // panicked while holding the guard (e.g. qr render OOM),
        // the PairState is still structurally valid — it's just
        // "observed while mid-mutation". Panicking here too would
        // turn a recoverable hiccup into a daemon crash.
        let mut state = self.pair_state.lock().unwrap_or_else(|p| p.into_inner());
        let new_nonce = crate::transport_iroh::generate_pair_nonce();
        let new_url = crate::transport_iroh::build_pairing_url_with_token(&state.addr, &new_nonce);
        let new_qr = crate::transport_iroh::render_qr_png_bytes(&new_url)?;
        state.nonce = Some(new_nonce);
        state.pairing_url = new_url;
        state.qr_png_bytes = new_qr;
        Ok(())
    }
}

impl Drop for EndpointHandle {
    fn drop(&mut self) {
        self._root_task.abort();
    }
}

/// The abstraction every daemon handler resolves through. Impls
/// must be `Send + Sync` (the daemon's tokio tasks hold them across
/// awaits) and have 'static lifetime (so an `Arc<dyn DaemonRegistry>`
/// can cross thread boundaries).
///
/// Renamed from `TerminalRegistry` (foundation task `another-one-ojm.1`):
/// the trait grew to host project / task / git / agent / MCP verbs
/// alongside the original terminal attach/resize methods. The seven
/// domain children of `another-one-ojm` (`.2..8`) extend it method-by-
/// method:
///
/// - `.2` — project mutation (`add_project`, `remove_project`).
/// - `.3` — task mutation (`create_task`, `rename_task`,
///   `set_task_pinned`, `remove_task`).
/// - `.4` — git state read (`read_changed_files`,
///   `read_recent_commits`, `read_branch_settings`).
/// - `.5` — git mutation (`stage_changed_file`, `discard_changed_file`,
///   `run_toolbar_git_action`, `create_branch`).
/// - `.6` — pull requests + checks (`find_project_pull_requests`,
///   `read_pull_request_checks`, `create_review_task`).
/// - `.7` — custom actions + Open In + agents (`list_project_actions`,
///   `run_project_action`, `read_open_in_state`).
/// - `.8` — settings (`read_git_action_scripts`, `set_shortcut_binding`,
///   `read_mcp_settings`).
///
/// This task only renames; the new methods land in their own PRs.
pub trait DaemonRegistry: Send + Sync + 'static {
    /// Snapshot of projects + tasks + tabs as of now. The daemon
    /// calls this on every `Control::ListProjects`, so cheap.
    fn list_projects(&self) -> Vec<ProjectSummary>;

    /// Subscribe to the live PTY byte stream for `(section_id,
    /// tab_id)`. Returns `None` if the tab isn't currently running
    /// (e.g., closed or never launched). Multiple subscribers share
    /// the same broadcast — each gets a fresh `Receiver`.
    fn attach_tab(&self, section_id: &str, tab_id: &str) -> Option<broadcast::Receiver<Vec<u8>>>;

    /// Feed input bytes into the tab's master PTY writer. Serialised
    /// by the underlying `Arc<Mutex<…>>` so desktop and mobile can
    /// both type concurrently without corrupting each other's bytes
    /// at the syscall level.
    fn tab_input(&self, section_id: &str, tab_id: &str, bytes: &[u8]);

    /// Announce that `viewer_id`'s viewport for this tab is
    /// `cols × rows`. The registry tracks every active viewer's
    /// preferred size and resizes the actual PTY to the **minimum**
    /// across them — a wide desktop window can't force the PTY into
    /// a column count the phone can't fit.
    ///
    /// `viewer_id` must be stable for the life of a single attached
    /// session (typical: remote EndpointId for iroh clients, a
    /// constant like `"desktop-local"` for in-process views).
    /// Clear stale entries with [`viewer_disconnected`] when a
    /// session ends.
    fn tab_resize(&self, viewer_id: &str, section_id: &str, tab_id: &str, cols: u16, rows: u16);

    /// Forget every size announcement this viewer made. Called when
    /// the viewer's session ends so its stale viewport doesn't keep
    /// clamping the PTY down forever.
    fn viewer_disconnected(&self, _viewer_id: &str) {}

    /// Launch the tab's PTY if it isn't already running. No-op if
    /// the tab is already live. After this returns, subsequent
    /// [`attach_tab`] calls for the same key should succeed (the
    /// actual launch may be async — clients may need to retry
    /// attach briefly). Default impl is a no-op for registries that
    /// can't launch (e.g. the sandbox binary's single-shell faker).
    fn launch_tab(&self, _section_id: &str, _tab_id: &str) {}

    /// Add an on-disk project at `path` to the daemon's store.
    /// Returns the freshly-inserted project's wire summary on
    /// success (so the iroh handler can emit it inline per the
    /// mutator-snapshot contract); errors are surfaced as
    /// `WorkerReply::Err` by the caller. Async because
    /// `prepare_project` does heavy disk + git work — bridging
    /// implementations dispatch to a background thread and `await`
    /// the result here.
    ///
    /// A path the store already knows is an error
    /// (`anyhow!("project at {path} already exists")`), not a
    /// silent no-op: the issuing client tried to add the same
    /// directory twice, so a typed failure is more honest than a
    /// fake-success Ack would be. Mirror of
    /// `another-one-bridge/src/api/local_session.rs::add_project`.
    fn add_project<'a>(&'a self, _path: String) -> RegistryFuture<'a, anyhow::Result<ProjectSummary>> {
        Box::pin(async { Err(anyhow::anyhow!("add_project: not supported on this registry")) })
    }

    /// Remove a project from the daemon's store by id. Cascades to
    /// the project's tasks + terminal sections (see
    /// [`another_one_core::project_store::ProjectStore::remove_project`]).
    /// Idempotent — passing an unknown id is silently a no-op, just
    /// like [`LocalSession::remove_project`]. Sync because the
    /// underlying store mutation doesn't touch the network or run
    /// any subprocess; the iroh handler can call this directly off
    /// its dispatch loop.
    fn remove_project(&self, _project_id: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!(
            "remove_project: not supported on this registry"
        ))
    }
}

/// A registry implementation suitable for the standalone sandbox
/// binary. Spawns one bash PTY per `attach_tab` call and treats the
/// single tab as the only tab that exists. Useful for smoke-testing
/// the iroh endpoint + mobile UI without the full desktop app
/// running; the desktop uses a different impl.
#[allow(dead_code)]
pub fn sandbox_registry() -> Arc<dyn DaemonRegistry> {
    Arc::new(crate::sandbox::SandboxRegistry::new())
}
