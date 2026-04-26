//! Trait the embedding process implements so the daemon library can
//! resolve projects/tasks/tabs and attach to live PTYs without
//! reaching into the desktop app's internals. Used by both the
//! standalone sandbox binary (where a minimal impl fakes a single
//! task + tab mapped to a throwaway shell) and by the desktop crate
//! (where the impl wraps `AnotherOneApp`'s real terminal runtimes).

use std::sync::{Arc, Mutex};

use iroh::EndpointAddr;
use tokio::sync::broadcast;

use crate::frame::ProjectSummary;

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

    // ── Git state read verbs (`another-one-ojm.4`) ─────────────────
    //
    // Sister methods to the per-verb signatures in
    // `another-one-bridge::api::local_session::LocalSession::*`. Each
    // returns the same shape (ignoring FRB-vs-wire DTO differences)
    // and follows the same `Ok(None) ⇒ unknown project` contract.
    //
    // Default impls forward to plumbing that doesn't need a real
    // project store (slugify) or return empty for sandbox registries
    // that can't answer. The bridge's `BridgeDaemonRegistry` overrides
    // each method with a real delegation as the verbs land.

    /// Compute the canonical branch slug for a free-text input.
    /// Pure — no project state involved. Default impl forwards to
    /// [`another_one_core::project_store::slugify_branch_name`].
    fn slugify_branch_name(&self, name: &str) -> String {
        another_one_core::project_store::slugify_branch_name(name)
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
