//! Trait the embedding process implements so the daemon library can
//! resolve projects/tasks/tabs and attach to live PTYs without
//! reaching into the desktop app's internals. Used by both the
//! standalone sandbox binary (where a minimal impl fakes a single
//! task + tab mapped to a throwaway shell) and by the desktop crate
//! (where the impl wraps `AnotherOneApp`'s real terminal runtimes).

use std::sync::{Arc, Mutex};

use iroh::EndpointAddr;
use tokio::sync::broadcast;

use crate::frame::{Check, ProjectPagePullRequest, ProjectSummary, PullRequestStatus};

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

    /// Resolve the latest pull-request status for `project_id`'s
    /// current branch. `Ok(None)` covers both "project not found"
    /// and "no PR for the branch"; `Err(_)` is reserved for hard
    /// failures (gh CLI missing, network) which the daemon then
    /// surfaces as [`crate::frame::WorkerReply::Err`].
    ///
    /// Default impl returns `Ok(None)` so the standalone sandbox
    /// can keep its in-memory shape (no real git host). The bridge
    /// override delegates to
    /// `another_one_core::git_actions::find_latest_pull_request_status`.
    fn find_pull_request_status(
        &self,
        _project_id: &str,
    ) -> Result<Option<PullRequestStatus>, String> {
        Ok(None)
    }

    /// Read CI checks attached to `project_id`'s current PR. Three-
    /// state return:
    ///   * `Ok(Some(list))` — PR exists, these are its check rows
    ///     (list may be empty when no checks are configured).
    ///   * `Ok(None)` — no PR for the current branch, or unknown
    ///     project.
    ///   * `Err(_)` — gh CLI missing, network failure, or any other
    ///     hard error. The daemon surfaces this as
    ///     [`crate::frame::WorkerReply::Err`] so the UI can render
    ///     a toast instead of a silent empty state.
    ///
    /// Default impl returns `Ok(None)` so the standalone sandbox's
    /// in-memory shape stays self-contained. The bridge override
    /// delegates to `another_one_core::git_actions::find_pull_request_checks`.
    fn read_pull_request_checks(
        &self,
        _project_id: &str,
    ) -> Result<Option<Vec<Check>>, String> {
        Ok(None)
    }

    /// Fetch open pull requests for `project_id` filtered by
    /// `filter_index` (0=all, 1=needs my review, 2=author:@me,
    /// 3=draft) plus an optional free-text `query`. `Ok(None)`
    /// means "unknown project id" so the UI can render its empty
    /// state; `Err(_)` is reserved for gh CLI / auth / network
    /// failures (surfaced upstream as
    /// [`crate::frame::WorkerReply::Err`]).
    ///
    /// Default impl returns `Ok(None)` so the sandbox keeps its
    /// in-memory shape. The bridge override delegates to
    /// `another_one_core::git_actions::find_project_pull_requests`
    /// (which shells out to `gh pr list`).
    fn find_project_pull_requests(
        &self,
        _project_id: &str,
        _filter_index: u32,
        _query: &str,
    ) -> Result<Option<Vec<ProjectPagePullRequest>>, String> {
        Ok(None)
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
