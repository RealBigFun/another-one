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

use daemon_proto::{
    ActiveGitStateWire, AgentProvider, AgentSettingsViewWire, BranchCompareFileWire,
    ChangedFileWire, Check, EnabledAgentsViewWire, GitActionScriptsView, McpSettingsView,
    OpenInSettingsViewWire, ProjectActionWire, ProjectPagePullRequest,
    ProjectSummary, PullRequestStatus, RecentCommitsWire, ResolvedBranchSettingsWire,
    ShortcutSettingsView, TaskSummary, ToolbarActionOutcome,
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

/// Live tab subscription plus any raw PTY bytes the client missed
/// before the subscription existed.
pub struct TabAttachment {
    pub replay: Vec<Vec<u8>>,
    pub receiver: broadcast::Receiver<Vec<u8>>,
}

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
    /// Same lifecycle as `_root_task` for the heartbeat-sweep
    /// task that ticks viewport liveness every 5 s.
    pub(crate) _sweep_task: tokio::task::AbortHandle,
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

    /// Direct socket addresses (`ip:port`) the iroh endpoint is
    /// reachable through. Snapshot taken when the endpoint bound; on a
    /// laptop changing networks these may go stale until a relay
    /// fallback or a re-bind. Used by the loopback-iroh bootstrap
    /// (`another-one-ojm.9`) to construct a same-process
    /// [`crate::api::iroh_client::iroh_connect`] target without
    /// round-tripping through the pairing URL.
    pub fn direct_addrs(&self) -> Vec<String> {
        self.pair_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .addr
            .ip_addrs()
            .map(|a| a.to_string())
            .collect()
    }

    /// Relay URLs the iroh endpoint is reachable through. Empty for
    /// the embedded daemon today (`presets::Minimal` skips relay
    /// publishing); included for shape-symmetry with the mobile pair
    /// path so the loopback bootstrap can pass the same shape.
    pub fn relay_urls(&self) -> Vec<String> {
        self.pair_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .addr
            .relay_urls()
            .map(|r| r.to_string())
            .collect()
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
        crate::transport_iroh::rotate_pair_state(&mut state)
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
    /// Return an error when the backing registry is unavailable. The
    /// transport checks this before dispatching control requests so
    /// an unhealthy embedder is reported as `WorkerReply::Err`
    /// instead of each verb degrading into empty/default responses.
    fn health(&self) -> Result<(), String> {
        Ok(())
    }

    /// Snapshot of projects + tasks + tabs as of now. The daemon
    /// calls this on every `Control::ListProjects`, so cheap.
    fn list_projects(&self) -> Vec<ProjectSummary>;

    /// Snapshot of per-repo metadata (branches, common git dir)
    /// alongside [`Self::list_projects`]. Bundled onto
    /// `WorkerReply::ProjectList` so clients can rebuild the
    /// sidebar's repo catalog without having to re-run git
    /// inspection locally — mobile can't, and desktop was silently
    /// wiping its own catalog on every projection push before this
    /// landed (see the sidebar-flicker regression around #125).
    ///
    /// Default impl returns an empty list for registries that
    /// don't track repo metadata (e.g. the sandbox binary); those
    /// clients just render projects without per-repo grouping,
    /// which is the same behaviour the old projection provided.
    fn list_repos(&self) -> Vec<daemon_proto::RepoSummary> {
        Vec::new()
    }

    /// Snapshot of the daemon's per-user UI state — pinned tasks,
    /// expanded sidebar repos, last focused section, etc. Bundled
    /// alongside `list_projects()` in the `WorkerReply::ProjectList`
    /// reply so both clients render the same state. Default impl
    /// returns an empty snapshot for registries that don't track UI
    /// state (e.g. the sandbox binary).
    fn ui_snapshot(&self) -> daemon_proto::UiSnapshot {
        daemon_proto::UiSnapshot::default()
    }

    /// Subscribe to state-change notifications. Each
    /// `Control::ListProjects` reply is built fresh from
    /// [`Self::list_projects`], so every fanned-out tick of the
    /// returned receiver tells server-side session loops "the
    /// projection has likely changed; push a fresh `ProjectList` to
    /// the peer". Default impl returns a never-yielding receiver
    /// for registries that don't track mutations (sandbox).
    fn subscribe_state_changes(&self) -> tokio::sync::broadcast::Receiver<()> {
        // Capacity 1 because this is "edge-triggered" — we drop
        // duplicate ticks; consumers re-snapshot on the leading
        // edge and ignore the rest.
        let (tx, rx) = tokio::sync::broadcast::channel(1);
        // Keep the sender alive for the lifetime of the receiver
        // so it doesn't get a `Closed` error immediately. Leak
        // intentionally for the no-op default — sandbox registries
        // never send.
        std::mem::forget(tx);
        rx
    }

    /// Notify every subscriber that the daemon's projection may
    /// have changed (project / task / tab mutation, settings tweak,
    /// pin toggle, etc.). Default impl is a no-op so registries
    /// that don't have mutation surfaces (e.g. the sandbox) are
    /// trivially compliant.
    fn notify_state_changed(&self) {}

    /// Persist the section's terminal-tab snapshot. `persisted` is
    /// an opaque JSON-serialised `PersistedSectionState`; the
    /// concrete registry deserialises and routes to either
    /// `update_task_tabs` (task-bound section) or
    /// `set_terminal_section` (project pages / standalone shells).
    /// Default impl is a no-op for registries that don't track
    /// section state.
    fn persist_section_state(&self, _section_id: &str, _persisted: serde_json::Value) {}

    /// Re-run the `gh auth status` probe and publish the new
    /// status through `UiSnapshot.gh_auth_status`. Fire-and-forget
    /// from the dispatch handler's PoV — the registry kicks a
    /// background worker (or no-op when not supported), then
    /// returns immediately. Default impl is a no-op for sandbox /
    /// test registries that have no `gh` to probe.
    fn recheck_gh_auth(&self) {}

    /// Update the user's last-active-section pointer. `None` clears
    /// it (no active section). Default impl is a no-op for
    /// registries that don't track UI state.
    fn set_last_active_section(&self, _section_id: Option<String>) {}

    /// Toggle the sidebar's per-task git-metadata visibility.
    /// Default impl is a no-op.
    fn set_sidebar_git_metadata_visible(&self, _visible: bool) {}

    /// Switch the app-wide theme preference. `mode_id` is the
    /// lowercase variant name from
    /// `core::project_store::ThemeMode` (`"light"` / `"dark"` /
    /// `"system"`). Default impl is a no-op so sandbox / test
    /// fakes don't have to carry theme state.
    fn set_theme_mode(&self, _mode_id: &str) {}

    /// Pin a repo's default commit action (`"commit"` /
    /// `"commit-and-push"`). Default impl is a no-op.
    fn set_repo_default_commit_action(&self, _repo_id: &str, _action: &str) {}

    /// Update a task's persisted branch name + target-project
    /// pointer (worktree-task move). Default impl is a no-op.
    fn update_task_branch(&self, _task_id: &str, _target_project_id: &str, _branch_name: &str) {}

    /// Replace the user's expanded-project set wholesale. Default
    /// impl is a no-op.
    fn set_expanded_repos(&self, _expanded_repo_ids: Vec<String>) {}

    /// Replace the AI commit-message generation LLM settings.
    /// `settings` is an opaque JSON-serialised
    /// `GitActionLlmSettings`. Default impl is a no-op.
    fn set_git_commit_llm(&self, _settings: serde_json::Value) {}

    /// Same as `set_git_commit_llm` but for PR generation. Default
    /// impl is a no-op.
    fn set_git_pr_llm(&self, _settings: serde_json::Value) {}

    /// Subscribe to the live PTY byte stream for `(section_id,
    /// tab_id)`. Returns `None` if the tab isn't currently running
    /// (e.g., closed or never launched). Multiple subscribers share
    /// the same broadcast — each gets a fresh `Receiver`.
    fn attach_tab(&self, section_id: &str, tab_id: &str) -> Option<broadcast::Receiver<Vec<u8>>>;

    /// Subscribe to a live tab and include any missed replay for this
    /// viewer. The default preserves existing registry semantics:
    /// subscribers get only future live bytes.
    fn attach_tab_with_replay(
        &self,
        _viewer_id: &str,
        section_id: &str,
        tab_id: &str,
    ) -> Option<TabAttachment> {
        self.attach_tab(section_id, tab_id)
            .map(|receiver| TabAttachment {
                replay: Vec::new(),
                receiver,
            })
    }

    /// Mark all currently-buffered output for this tab as observed by
    /// the viewer. Called when a live attachment is replaced/detached;
    /// registries without replay can ignore it.
    fn note_tab_output_observed(&self, _viewer_id: &str, _section_id: &str, _tab_id: &str) {}

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

    /// Refresh the liveness timestamp for this viewer. The daemon
    /// uses it to detect viewers that went silent without a clean
    /// disconnect (backgrounded phone, network flake, process
    /// kill) and sweep their viewport claims via
    /// [`sweep_stale_viewers`]. Default impl is a no-op for
    /// registries that don't need liveness tracking.
    fn note_viewer_heartbeat(&self, _viewer_id: &str) {}

    /// Remove viewers whose last activity is older than
    /// `stale_ms`. Called periodically by the daemon's accept loop.
    /// Returns the set of tab keys whose `effective_size` may have
    /// changed so callers (the desktop's render tick) can rerun
    /// the resize pump. Default impl is a no-op.
    fn sweep_stale_viewers(&self, _stale_ms: u64) {}

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
    /// `prepare_project` does heavy disk + git work — production
    /// implementations dispatch to a background thread and `await`
    /// the result here.
    ///
    /// A path the store already knows is an error
    /// (`anyhow!("project at {path} already exists")`), not a
    /// silent no-op: the issuing client tried to add the same
    /// directory twice, so a typed failure is more honest than a
    /// fake-success Ack would be.
    fn add_project<'a>(
        &'a self,
        _path: String,
    ) -> RegistryFuture<'a, anyhow::Result<ProjectSummary>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "add_project: not supported on this registry"
            ))
        })
    }

    /// Remove a project from the daemon's store by id. Cascades to
    /// the project's tasks + terminal sections (see
    /// [`another_one_core::project_store::ProjectStore::remove_project`]).
    /// Idempotent — passing an unknown id is silently a no-op, just
    /// like the original local desktop path. Sync because the
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
    // Mirror of the UI-facing task mutation methods. Heavy ones
    // return `RegistryFuture` so the embedder can spawn worker
    // threads and `.await` them; the lightweight ones (`rename`,
    // `set_pinned`, `remove`) are sync because the implementations
    // are also sync after the registry lock is taken.

    /// Create a worktree task on `project_id`. Returns the inserted
    /// task's [`TaskSummary`] — the caller wraps it in
    /// [`crate::frame::WorkerReply::CreateWorktreeTaskAck`]. The future runs
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
    // Sister methods to the UI-facing project/git operations. Each
    // returns the same shape and follows the same `Ok(None) ⇒ unknown
    // project` contract.
    //
    // Default impls forward to plumbing that doesn't need a real
    // project store (slugify) or return empty for sandbox registries
    // that can't answer. Production registry implementations override
    // each method with a real delegation as the verbs land.

    /// Branch names available on `project_id`'s git repo. Empty
    /// list for unknown projects.
    fn read_project_branches(&self, _project_id: &str) -> Vec<String> {
        Vec::new()
    }

    /// Default branch the new-task modal seeds for `project_id`.
    /// Returns `None` when the project has no current branch yet
    /// (fresh repo).
    fn primary_branch_for_project(&self, _project_id: &str) -> Option<String> {
        None
    }

    /// User's preferred default commit action for `project_id`'s
    /// root repo. Returns `"commit"` / `"commit-and-push"` / `None`.
    fn repo_default_commit_action(&self, _project_id: &str) -> Option<String> {
        None
    }

    /// Snapshot the active project's branch metadata — current
    /// branch name + ahead / behind counts. May shell out to git;
    /// implementations should arrange for `block_in_place` or
    /// `spawn_blocking` so the daemon's tokio worker isn't held
    /// across the syscalls.
    fn read_active_git_state(&self, _project_id: &str) -> Option<ActiveGitStateWire> {
        None
    }

    /// Working-tree changes for `project_id`. Returns `None` for
    /// unknown project ids.
    fn read_changed_files(&self, _project_id: &str) -> Option<Vec<ChangedFileWire>> {
        None
    }

    /// Resolve `project_id`'s GitHub remote URL. Returns `None` for
    /// unknown projects, projects without an `origin`, or non-
    /// github.com remotes.
    fn read_project_github_url(&self, _project_id: &str) -> Option<String> {
        None
    }

    /// Recent commits on `project_id`'s current branch, capped at
    /// `limit`. Returns `None` for unknown project ids; `Err` for
    /// git failures (commit pruned, etc.). May shell out to git.
    fn read_recent_commits(
        &self,
        _project_id: &str,
        _limit: usize,
    ) -> Result<Option<RecentCommitsWire>, String> {
        Ok(None)
    }

    /// Per-commit file-change list. Returns `None` for unknown
    /// project ids; `Err` for git failures (commit pruned, etc.).
    fn read_commit_file_changes(
        &self,
        _project_id: &str,
        _commit_id: &str,
    ) -> Result<Option<Vec<BranchCompareFileWire>>, String> {
        Ok(None)
    }

    /// Snapshot the resolved branch settings for `project_id`'s
    /// root project. Returns `None` for unknown / repo-less projects.
    fn read_branch_settings(&self, _project_id: &str) -> Option<ResolvedBranchSettingsWire> {
        None
    }

    /// Update one branch-setting field. `field` is `"default-branch"`
    /// or `"default-target-branch"`; `branch_name` of `None` clears
    /// the override. Returns `Ok(true)` when the persisted store
    /// changed.
    fn set_branch_setting(
        &self,
        _project_id: &str,
        _field: &str,
        _branch_name: Option<&str>,
    ) -> Result<bool, String> {
        Err("set_branch_setting: not supported by this registry".to_string())
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

    /// `another-one-ojm.5` — discard a batch of changed files and
    /// return the final `changed_files` snapshot plus any per-path
    /// failures.
    fn discard_all_changes<'a>(
        &'a self,
        _project_id: &'a str,
        _files: Vec<ChangedFileWire>,
    ) -> RegistryFuture<'a, anyhow::Result<(Vec<ChangedFileWire>, Vec<String>)>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "discard_all_changes: not supported on this registry"
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
    /// can keep its in-memory shape (no real git host). Production
    /// implementations delegate to
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
    /// in-memory shape stays self-contained. Production implementations
    /// delegate to `another_one_core::git_actions::find_pull_request_checks`.
    fn read_pull_request_checks(&self, _project_id: &str) -> Result<Option<Vec<Check>>, String> {
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
    /// in-memory shape. Production implementations delegate to
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

    /// Submit the new-task modal. Returns the section id the caller
    /// should focus. Default impl returns `Err("unsupported")`.
    fn submit_new_task(
        &self,
        _project_id: String,
        _task_name: String,
        _source_branch: String,
        _agent_ids: Vec<String>,
        _branch_mode_existing: bool,
        _worktree_mode: bool,
    ) -> RegistryFuture<'_, anyhow::Result<String>> {
        Box::pin(async { Err(anyhow::anyhow!("unsupported on this daemon")) })
    }

    /// Append one agent tab (or plain shell when `agent_id` is
    /// empty) to an existing section. Returns the new tab id.
    fn add_agent_to_section(&self, _section_id: &str, _agent_id: &str) -> Result<String, String> {
        Err("unsupported on this daemon".to_string())
    }

    /// Persist the active tab for a section.
    fn activate_section_tab(&self, _section_id: &str, _tab_id: &str) -> Result<(), String> {
        Err("unsupported on this daemon".to_string())
    }

    /// Remove a tab from a section. Returns the new active tab id, or
    /// empty when the section is now tabless.
    fn close_section_tab(&self, _section_id: &str, _tab_id: &str) -> Result<String, String> {
        Err("unsupported on this daemon".to_string())
    }

    /// Set one section tab's `pinned` flag to a specific value
    /// (idempotent). Returns the applied value (which equals the
    /// `pinned` arg unless the tab/section couldn't be resolved
    /// and an error is returned).
    fn set_section_tab_pinned(
        &self,
        _section_id: &str,
        _tab_id: &str,
        _pinned: bool,
    ) -> Result<bool, String> {
        Err("unsupported on this daemon".to_string())
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

    /// Toggle one agent's enabled flag in the daemon host config.
    fn set_agent_enabled(&self, _agent_id: &str, _enabled: bool) -> Result<bool, String> {
        Err("unsupported on this daemon".to_string())
    }

    /// Mark an enabled agent as the daemon host's default.
    fn set_default_agent(&self, _agent_id: &str) -> Result<bool, String> {
        Err("unsupported on this daemon".to_string())
    }

    /// Replace one agent's launch-args list. Empty args clear the
    /// override.
    fn set_agent_launch_args(&self, _agent_id: &str, _args: Vec<String>) -> Result<bool, String> {
        Err("unsupported on this daemon".to_string())
    }

    /// Snapshot of the Settings → Open In page on the daemon host.
    /// Default impl returns `None` for registries that do not surface
    /// Open-In settings.
    fn read_open_in_settings(&self) -> Option<OpenInSettingsViewWire> {
        None
    }

    /// Toggle one Open-In app's enabled flag. Default impl returns
    /// `Err("unsupported")`.
    fn set_open_in_app_enabled(&self, _app_id: &str, _enabled: bool) -> Result<(), String> {
        Err("unsupported on this daemon".to_string())
    }

    /// Launch a project directory in a host-local app. Default impl
    /// returns `Err("unsupported")`.
    fn open_project_in_app(&self, _project_id: &str, _app_id: &str) -> Result<(), String> {
        Err("unsupported on this daemon".to_string())
    }

    /// Run one custom action inside `section_id`'s task. Returns
    /// the freshly-minted tab id on success, or a human-readable
    /// error on failure (unknown project / action id, malformed
    /// section id, empty shell command, etc.).
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

    /// Upsert one custom action. Default impl returns `Err("unsupported")`.
    fn save_project_action(
        &self,
        _project_id: &str,
        _action: ProjectActionWire,
        _save_global_copy: bool,
    ) -> Result<(), String> {
        Err("unsupported on this daemon".to_string())
    }

    /// Delete one custom action by id. Default impl returns `false`.
    fn delete_project_action(&self, _project_id: &str, _action_id: &str) -> bool {
        false
    }

    // ── Settings → Git Actions (`another-one-ojm.8`) ───────────────

    /// Snapshot of the Settings → Git Actions page state. Default
    /// returns an empty/default view for registries that don't surface settings
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

    /// Snapshot of the Settings → Keybindings page. Default returns
    /// an empty list for registries that don't surface settings.
    fn read_shortcut_settings(&self) -> ShortcutSettingsView {
        ShortcutSettingsView {
            actions: Vec::new(),
        }
    }

    /// Set / clear one shortcut binding. Empty `binding` clears the
    /// action. Returns `Err` for unknown action ids — the daemon
    /// surfaces those as `WorkerReply::Err { kind: UnknownId }`.
    fn set_shortcut_binding(&self, _action_id: &str, _binding: &str) -> Result<(), String> {
        Err("not supported on sandbox".to_string())
    }

    /// Reset one shortcut to its built-in default.
    fn reset_shortcut_binding(&self, _action_id: &str) -> Result<(), String> {
        Err("not supported on sandbox".to_string())
    }

    // ── Settings → MCP (`another-one-ojm.8`) ───────────────────────

    /// Snapshot of the catalog + on-disk MCP registry. Default
    /// returns empty lists for registries that don't surface settings.
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
