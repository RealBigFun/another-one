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

use crate::frame::{
    ActiveGitStateWire, AgentProvider, AgentSettingsViewWire, BranchCompareFileWire,
    ChangedFileWire, Check, EnabledAgentsViewWire, GitActionScriptsView, McpSettingsView,
    OpenInStateWire, ProjectActionWire, ProjectPagePullRequest, ProjectSummary,
    PullRequestStatus, RecentCommitsWire, ShortcutSettingsView, TaskSummary,
    ToolbarActionOutcome,
};

/// Boxed-future return type for `DaemonRegistry` methods that are
/// async on the embedder side (spawn a worker thread + await its
/// reply, etc.).
///
/// `DaemonRegistry` is a trait object (callers hold
/// `Arc<dyn DaemonRegistry>`), so methods can't be `async fn`
/// directly — that desugars to a per-impl `impl Future` which isn't
/// object-safe ahead of the dyn-async-fn-in-trait stabilisation.
/// Pinned + boxed futures keep the trait dyn-compatible without an
/// `async-trait`-style hidden allocation pattern.
///
/// `'a` is the borrow of `&self` the method took when it produced
/// the future. Most embedder impls clone `Arc`s or upgrade `Weak`s
/// and own the result (no borrow across the await); `'a` is there
/// so an impl CAN borrow if it wants to.
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

    // ── Project mutation (another-one-ojm.2) ──────────────────────

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

    // ── Task mutation (another-one-ojm.3) ─────────────────────────
    //
    // Mirror of `LocalSession`'s task mutation methods. Heavy ones
    // return `RegistryFuture` so the embedder can spawn worker
    // threads and `.await` them; the lightweight ones (`rename`,
    // `set_pinned`, `remove`) are sync because the FRB caller's
    // implementations are also sync after the registry lock is
    // taken.

    /// Create a worktree task on `project_id`. Returns the inserted
    /// task's [`TaskSummary`] — the caller wraps it in
    /// [`crate::frame::WorkerReply::TaskCreated`]. The future runs
    /// the heavy `core::project_service::spawn_task_creation` worker
    /// thread under the hood; clients can expect tens of seconds
    /// before resolution. Default impl returns an `unsupported`
    /// error so a sandbox / test registry doesn't have to stub
    /// every domain method.
    fn create_worktree_task(
        &self,
        _project_id: String,
        _task_name: String,
        _source_branch: String,
        _agent_provider: Option<AgentProvider>,
    ) -> RegistryFuture<'_, anyhow::Result<TaskSummary>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "create_worktree_task: registry impl does not support task creation"
            ))
        })
    }

    /// Rename a task. Returns `(changed, task)`: `changed` is `false`
    /// for an unknown id or a no-op rename; `task` is the post-
    /// rename snapshot when the task exists, `None` for an unknown
    /// id. Default returns `(false, None)`.
    fn rename_task(&self, _task_id: &str, _new_name: &str) -> (bool, Option<TaskSummary>) {
        (false, None)
    }

    /// Pin or unpin a task. Returns `(changed, task)`: `changed` is
    /// `false` for an idempotent re-set, `task` is the post-
    /// mutation snapshot. Default returns `(false, None)`.
    fn set_task_pinned(&self, _task_id: &str, _pinned: bool) -> (bool, Option<TaskSummary>) {
        (false, None)
    }

    /// Remove a task and its sections. Returns whether anything was
    /// actually removed (idempotent for unknown ids). Default
    /// returns `false`.
    fn remove_task(&self, _project_id: &str, _task_id: &str) -> bool {
        false
    }

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

    /// Branch names available on `project_id`'s git repo. Empty
    /// list for unknown projects. Sister to
    /// `LocalSession::read_project_branches`.
    fn read_project_branches(&self, _project_id: &str) -> Vec<String> {
        Vec::new()
    }

    /// Default branch the new-task modal seeds for `project_id`.
    /// Returns `None` when the project has no current branch yet
    /// (fresh repo). Sister to
    /// `LocalSession::primary_branch_for_project`.
    fn primary_branch_for_project(&self, _project_id: &str) -> Option<String> {
        None
    }

    /// User's preferred default commit action for `project_id`'s
    /// root repo. Returns `"commit"` / `"commit-and-push"` / `None`.
    /// Sister to `LocalSession::repo_default_commit_action`.
    fn repo_default_commit_action(&self, _project_id: &str) -> Option<String> {
        None
    }

    /// Snapshot the active project's branch metadata — current
    /// branch name + ahead / behind counts. Sister to
    /// `LocalSession::read_active_git_state`. May shell out to git;
    /// implementations should arrange for `block_in_place` or
    /// `spawn_blocking` so the daemon's tokio worker isn't held
    /// across the syscalls.
    fn read_active_git_state(&self, _project_id: &str) -> Option<ActiveGitStateWire> {
        None
    }

    /// Working-tree changes for `project_id`. Sister to
    /// `LocalSession::read_changed_files`. Returns `None` for
    /// unknown project ids.
    fn read_changed_files(&self, _project_id: &str) -> Option<Vec<ChangedFileWire>> {
        None
    }

    /// Resolve `project_id`'s GitHub remote URL. Returns `None` for
    /// unknown projects, projects without an `origin`, or non-
    /// github.com remotes. Sister to
    /// `LocalSession::read_project_github_url`.
    fn read_project_github_url(&self, _project_id: &str) -> Option<String> {
        None
    }

    /// Recent commits on `project_id`'s current branch, capped at
    /// `limit`. Returns `None` for unknown project ids; `Err` for
    /// git failures (commit pruned, etc.). Sister to
    /// `LocalSession::read_recent_commits`. May shell out to git.
    fn read_recent_commits(
        &self,
        _project_id: &str,
        _limit: usize,
    ) -> Result<Option<RecentCommitsWire>, String> {
        Ok(None)
    }

    /// Per-commit file-change list. Returns `None` for unknown
    /// project ids; `Err` for git failures (commit pruned, etc.).
    /// Sister to `LocalSession::read_commit_file_changes`.
    fn read_commit_file_changes(
        &self,
        _project_id: &str,
        _commit_id: &str,
    ) -> Result<Option<Vec<BranchCompareFileWire>>, String> {
        Ok(None)
    }

    // ── Git mutation (another-one-ojm.5) ──────────────────────────

    /// `another-one-ojm.5` — stage one changed file via `git add -A`.
    /// `original_path` is `Some(_)` only on rename/copy entries.
    /// Returns the post-mutation `changed_files` snapshot so the
    /// caller's ack can carry it inline (per the inline-snapshot
    /// contract in `frame.rs`).
    fn stage_changed_file<'a>(
        &'a self,
        _project_id: &'a str,
        _path: &'a str,
        _original_path: Option<&'a str>,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "stage_changed_file: not supported on this registry"
            ))
        })
    }

    /// `another-one-ojm.5` — unstage one changed file. Same
    /// inline-snapshot return shape as [`Self::stage_changed_file`].
    fn unstage_changed_file<'a>(
        &'a self,
        _project_id: &'a str,
        _path: &'a str,
        _original_path: Option<&'a str>,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "unstage_changed_file: not supported on this registry"
            ))
        })
    }

    /// `another-one-ojm.5` — `git add -A` on the project root.
    /// Returns the post-mutation `changed_files` snapshot for the
    /// caller's inline-snapshot ack.
    fn stage_all_changes<'a>(
        &'a self,
        _project_id: &'a str,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "stage_all_changes: not supported on this registry"
            ))
        })
    }

    /// `another-one-ojm.5` — unstage every staged change in one shot.
    fn unstage_all_changes<'a>(
        &'a self,
        _project_id: &'a str,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "unstage_all_changes: not supported on this registry"
            ))
        })
    }

    /// `another-one-ojm.5` — discard one file's working-tree changes.
    /// `untracked` is passed verbatim to the core helper; rename pairs
    /// surface via `original_path`.
    fn discard_changed_file<'a>(
        &'a self,
        _project_id: &'a str,
        _path: &'a str,
        _untracked: bool,
        _original_path: Option<&'a str>,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "discard_changed_file: not supported on this registry"
            ))
        })
    }

    /// `another-one-ojm.5` — run one of the titlebar git actions.
    /// `action_id` strings round-trip verbatim from the wire (see
    /// [`crate::frame::Control::RunToolbarGitAction`]).
    fn run_toolbar_git_action<'a>(
        &'a self,
        _project_id: &'a str,
        _action_id: &'a str,
    ) -> RegistryFuture<'a, anyhow::Result<ToolbarActionOutcome>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "run_toolbar_git_action: not supported on this registry"
            ))
        })
    }

    /// `another-one-ojm.5` — create a branch from HEAD. Returns the
    /// new task's `section_id` (or empty string for the current-task
    /// case) plus the post-mutation `projects` snapshot for the
    /// caller's inline-snapshot ack.
    fn create_branch<'a>(
        &'a self,
        _project_id: &'a str,
        _branch_name: &'a str,
        _use_current_task: bool,
        _migrate_changes: bool,
    ) -> RegistryFuture<'a, anyhow::Result<(String, Vec<ProjectSummary>)>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "create_branch: not supported on this registry"
            ))
        })
    }

    /// `another-one-ojm.5` — spawn a review task targeting a PR.
    /// Returns the new task's `section_id` plus the post-mutation
    /// `projects` snapshot, same shape as [`Self::create_branch`].
    fn create_review_task<'a>(
        &'a self,
        _project_id: &'a str,
        _pull_request_number: u64,
        _head_branch: &'a str,
        _agent_provider: Option<AgentProvider>,
    ) -> RegistryFuture<'a, anyhow::Result<(String, Vec<ProjectSummary>)>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "create_review_task: not supported on this registry"
            ))
        })
    }

    // ── Pull requests + checks (another-one-ojm.6) ─────────────────

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

    // ── Custom actions + Open In + agents (another-one-ojm.7) ─────

    /// Snapshot of the host's "Open In" config — installed-and-enabled
    /// apps + preferred default. Read-only (the actual `xdg-open`
    /// spawn stays host-local on the daemon, by design — see
    /// `connection.dart::openProjectInApp` for why). Default impl
    /// returns `None` for registries that don't surface Open-In
    /// (the sandbox binary has no host editor detection).
    fn open_in_state(&self) -> Option<OpenInStateWire> {
        None
    }

    /// Project + global custom actions for `project_id`, in the same
    /// dropdown order GPUI's titlebar split-button renders. Empty
    /// list when the project is unknown. Default impl returns empty
    /// (the sandbox binary has no project store).
    fn list_project_actions(&self, _project_id: &str) -> Vec<ProjectActionWire> {
        Vec::new()
    }

    /// Snapshot of agents the user has enabled on this host plus
    /// the id of the one they've picked as default. Drives the
    /// new-task modal's agent multi-select; the order is the
    /// canonical `core::agents::AGENTS` order so the UI can render
    /// without re-sorting. Default impl returns an empty view —
    /// the sandbox binary has no agents config to surface.
    fn read_enabled_agents(&self) -> EnabledAgentsViewWire {
        EnabledAgentsViewWire {
            agents: Vec::new(),
            default_agent_id: None,
        }
    }

    /// Full agent registry — every entry in `core::agents::AGENTS`
    /// paired with per-host enabled / default flags + per-agent
    /// launch-args list. Drives the Settings → Agents page on a
    /// remote client. Default impl returns an empty view; the
    /// sandbox binary has no agents config.
    fn read_agent_settings(&self) -> AgentSettingsViewWire {
        AgentSettingsViewWire {
            agents: Vec::new(),
            default_agent_id: None,
        }
    }

    /// Run one custom action inside `section_id`'s task. Returns
    /// the freshly-minted tab id on success, or a human-readable
    /// error on failure (unknown project / action id, malformed
    /// section id, empty shell command, etc. — matches
    /// `LocalSession::run_project_action`).
    ///
    /// Single-shot Ack semantics: the action's PTY output flows
    /// over the existing `Control::AttachTab` pipeline; this verb
    /// only kicks off the spawn. Default impl returns
    /// `Err("unsupported")` for registries with no project store
    /// to mutate (the sandbox binary).
    fn run_project_action(
        &self,
        _project_id: &str,
        _section_id: &str,
        _action_id: &str,
    ) -> Result<String, String> {
        Err("unsupported on this daemon".to_string())
    }

    // ── Settings → Git Actions (`another-one-ojm.8`) ───────────────

    /// Snapshot of the Settings → Git Actions page state. Mirrors
    /// `LocalSession::read_git_action_scripts`. Default returns an
    /// empty/default view for registries that don't surface settings
    /// (the standalone sandbox).
    fn read_git_action_scripts(&self) -> GitActionScriptsView {
        GitActionScriptsView {
            commit_script: String::new(),
            commit_using_default: true,
            pr_script: String::new(),
            pr_using_default: true,
        }
    }

    /// Replace the commit-message generation script. Returns whether
    /// the on-disk store changed. Default `Err("not supported on
    /// sandbox")`.
    fn set_git_commit_script(&self, _script: &str) -> Result<bool, String> {
        Err("not supported on sandbox".to_string())
    }

    /// Drop the commit-script override and revert to the built-in
    /// default. Returns whether anything was removed.
    fn reset_git_commit_script(&self) -> Result<bool, String> {
        Err("not supported on sandbox".to_string())
    }

    /// Replace the PR title/body generation script. Returns whether
    /// the on-disk store changed.
    fn set_git_pr_script(&self, _script: &str) -> Result<bool, String> {
        Err("not supported on sandbox".to_string())
    }

    /// Drop the PR-script override and revert to the built-in
    /// default. Returns whether anything was removed.
    fn reset_git_pr_script(&self) -> Result<bool, String> {
        Err("not supported on sandbox".to_string())
    }

    // ── Settings → Keybindings (`another-one-ojm.8`) ───────────────

    /// Snapshot of the Settings → Keybindings page. Mirrors
    /// `LocalSession::read_shortcut_settings`. Default returns an
    /// empty list for registries that don't surface settings.
    fn read_shortcut_settings(&self) -> ShortcutSettingsView {
        ShortcutSettingsView {
            actions: Vec::new(),
        }
    }

    /// Set / clear one shortcut binding. Empty `binding` clears the
    /// action. Returns `Err` for unknown action ids — the daemon
    /// surfaces those as `WorkerReply::Err { kind: UnknownId }`.
    fn set_shortcut_binding(
        &self,
        _action_id: &str,
        _binding: &str,
    ) -> Result<(), String> {
        Err("not supported on sandbox".to_string())
    }

    /// Reset one shortcut to its built-in default.
    fn reset_shortcut_binding(&self, _action_id: &str) -> Result<(), String> {
        Err("not supported on sandbox".to_string())
    }

    // ── Settings → MCP (`another-one-ojm.8`) ───────────────────────

    /// Snapshot of the catalog + on-disk MCP registry. Mirrors
    /// `LocalSession::read_mcp_settings`. Default returns empty lists
    /// for registries that don't surface settings.
    fn read_mcp_settings(&self) -> McpSettingsView {
        McpSettingsView {
            catalog_entries: Vec::new(),
            registry_entries: Vec::new(),
            sync_error_provider_ids: Vec::new(),
        }
    }

    /// Add one catalog entry to the registry. No-op when the id
    /// isn't a known catalog id or the entry's already in the
    /// registry.
    fn mcp_add_from_catalog(&self, _catalog_id: &str) -> Result<(), String> {
        Err("not supported on sandbox".to_string())
    }

    /// Toggle one entry's enabled flag for one provider. Runs
    /// `sync_all` on success. Returns `Err` for unknown provider ids
    /// (surfaced as `WorkerReply::Err { kind: UnknownId }`).
    fn mcp_toggle(
        &self,
        _entry_id: &str,
        _provider_id: &str,
        _enabled: bool,
    ) -> Result<(), String> {
        Err("not supported on sandbox".to_string())
    }

    /// Remove one entry from the registry. Runs `sync_all` on
    /// success.
    fn mcp_remove(&self, _entry_id: &str) -> Result<(), String> {
        Err("not supported on sandbox".to_string())
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
