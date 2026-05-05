//! Transport-agnostic verb dispatch.
//!
//! The legacy `transport_iroh::handle_control` function did verb
//! dispatch *inside* the iroh frame loop — `outbound_tx`, attach-
//! state forwarding, and request-id correlation were all interleaved.
//! That worked for one transport but blocked the abstract-`ServerSession`
//! cutover the daemon-transport epic (`another-one-iem`) is driving
//! toward.
//!
//! This module is the seam: a [`serve_session`] entry point that
//! loops on a [`ServerSession::next_call`], dispatches by `Control`
//! variant, and emits replies via [`ServerSession::reply`]. No iroh
//! types, no `OutboundTx`, no transport-specific knobs — just the
//! abstract trait surface plus the registry.
//!
//! ## Attach state
//!
//! The per-connection attach state (zero or one currently-attached
//! `(section_id, tab_id)`) lives here as [`AttachState`]. The PTY
//! forwarder task — which pumps a [`broadcast::Receiver<Vec<u8>>`]
//! into [`ServerSession::push_data`] — is also spawned here. Concrete
//! transports never see it; they only see `push_data` calls.
//!
//! Forwarder generation gating: every attach increments a per-session
//! generation counter. Each forwarder captures its own generation and
//! checks before each push that it still matches the live one. On
//! detach / re-attach the counter advances and any in-flight bytes
//! from the previous forwarder are dropped on the floor. This closes
//! the race where `AbortHandle::abort()` returns before the spawned
//! task has actually been cancelled.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use daemon_proto::{Control, ErrKind, WorkerReply};
use daemon_transport::{ServerSession, TransportError};
use tokio::sync::broadcast;
use tokio::task::AbortHandle;
use tracing::{debug, warn};

use crate::registry::DaemonRegistry;

/// Per-session attach bookkeeping. `serve_session` owns one of these
/// per active session and threads it through every `dispatch_call`.
/// Concrete transports never touch it directly.
#[derive(Default)]
pub struct AttachState {
    inner: Mutex<AttachInner>,
    /// Monotonically-increasing forwarder generation. Each `AttachTab`
    /// bumps it (unless re-attaching to the same target); each
    /// forwarder task captures its generation at spawn and checks
    /// before pushing.
    generation: AtomicU64,
}

#[derive(Default)]
struct AttachInner {
    section_id: Option<String>,
    tab_id: Option<String>,
    /// Abort handle for the forwarder task pumping the broadcast
    /// receiver into `session.push_data`. `None` when no live
    /// runtime was available at AttachTab time (the registry returned
    /// no receiver) — the attach is recorded but no forwarder is
    /// spawned.
    forwarder: Option<AbortHandle>,
}

impl AttachState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the current attach target as `(section_id, tab_id)`,
    /// or `None` if the session has no attached tab. Concrete
    /// transports use this to route inbound raw-bytes frames (e.g.
    /// the iroh transport's `TY_DATA` frames) into
    /// `registry.tab_input`.
    pub fn snapshot_target(&self) -> Option<(String, String)> {
        let inner = self.inner.lock().expect("attach state poisoned");
        match (&inner.section_id, &inner.tab_id) {
            (Some(s), Some(t)) => Some((s.clone(), t.clone())),
            _ => None,
        }
    }

    fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Tear down the active forwarder (if any) and clear the attach
    /// target. Bumps the generation so any in-flight forwarder push
    /// is rejected. Returns the prior `(section_id, tab_id)` when one
    /// was set so callers can call `note_tab_output_observed`.
    fn detach(&self) -> Option<(String, String, bool)> {
        self.generation.fetch_add(1, Ordering::Relaxed);
        let mut inner = self.inner.lock().expect("attach state poisoned");
        let prior = match (inner.section_id.take(), inner.tab_id.take()) {
            (Some(s), Some(t)) => Some((s, t, inner.forwarder.is_some())),
            _ => None,
        };
        if let Some(handle) = inner.forwarder.take() {
            handle.abort();
        }
        prior
    }
}

/// Drive `session` against `registry` until the peer closes. Pulls
/// verbs via [`ServerSession::next_call`], dispatches by variant,
/// emits the matching reply via [`ServerSession::reply`].
///
/// Returns when `next_call` yields `Ok(None)` (clean close) or any
/// step errors. The caller is responsible for tearing the session
/// down and logging the outcome.
pub async fn serve_session(
    session: Arc<dyn ServerSession>,
    registry: Arc<dyn DaemonRegistry>,
) -> Result<(), TransportError> {
    serve_session_with_attach(session, registry, Arc::new(AttachState::new())).await
}

/// Variant of [`serve_session`] that uses a pre-existing
/// [`AttachState`]. Concrete transports that need to mirror the attach
/// target (e.g. the iroh transport routes inbound `TY_DATA` to
/// `registry.tab_input` based on the live attach key) construct the
/// `AttachState` themselves so they can read it from the frame loop.
pub async fn serve_session_with_attach(
    session: Arc<dyn ServerSession>,
    registry: Arc<dyn DaemonRegistry>,
    attach: Arc<AttachState>,
) -> Result<(), TransportError> {
    let viewer_id = session.peer_id().to_string();
    let attach_for_loop = Arc::clone(&attach);
    let result = loop {
        let next = match session.next_call().await {
            Ok(v) => v,
            Err(e) => break Err(e),
        };
        let Some((request_id, ctrl)) = next else {
            break Ok(());
        };
        let reply = dispatch_call(
            ctrl,
            registry.as_ref(),
            session.as_ref(),
            session.clone(),
            attach_for_loop.as_ref(),
            attach_for_loop.clone(),
            &viewer_id,
        )
        .await;
        if let Some(reply) = reply {
            if let Err(e) = session.reply(request_id, reply).await {
                break Err(e);
            }
        }
    };

    // Tear down any lingering attach state on the way out so the
    // forwarder task doesn't outlive the session.
    if let Some((section_id, tab_id, had_forwarder)) = attach.detach() {
        if had_forwarder {
            registry.note_tab_output_observed(&viewer_id, &section_id, &tab_id);
        }
    }
    registry.viewer_disconnected(&viewer_id);
    result
}

/// Map a single `Control` verb to a `WorkerReply`. Returns `None` for
/// the rare verbs that don't elicit a reply (`WatchProject` legacy
/// no-op, `LaunchTab`, `Resize`/`TabResize`, `DetachTab`) — the
/// caller skips `session.reply` for those.
#[allow(clippy::too_many_arguments)]
async fn dispatch_call(
    ctrl: Control,
    registry: &dyn DaemonRegistry,
    session: &dyn ServerSession,
    session_arc: Arc<dyn ServerSession>,
    attach: &AttachState,
    attach_arc: Arc<AttachState>,
    viewer_id: &str,
) -> Option<WorkerReply> {
    // The iroh transport gates every non-Hello verb on a
    // `registry.health()` check (one source of truth for "registry
    // dropped" — typically when the desktop app is quitting). Match
    // that here so the abstract dispatch path doesn't accidentally
    // route verbs into a half-shutdown registry.
    if !matches!(ctrl, Control::Hello { .. }) {
        if let Err(message) = registry.health() {
            return Some(WorkerReply::Err {
                kind: ErrKind::Internal,
                message,
            });
        }
    }

    // Per-domain helpers that pre-handle a contiguous set of
    // variants. Each returns `Ok(reply)` if it consumed the verb and
    // `Err(ctrl)` to pass it on. Mirrors the legacy
    // `transport_iroh::handle_control` flow so adding a verb to
    // either path lights up automatically here too.
    let ctrl = match crate::commands::agent_settings::handle(ctrl, registry) {
        Ok(reply) => return Some(reply),
        Err(ctrl) => ctrl,
    };
    let ctrl = match crate::commands::open_in::handle(ctrl, registry) {
        Ok(reply) => return Some(reply),
        Err(ctrl) => ctrl,
    };

    match ctrl {
        // ── Attach lifecycle ─────────────────────────────────────────
        Control::AttachTab { section_id, tab_id } => {
            handle_attach(
                section_id,
                tab_id,
                registry,
                session,
                session_arc,
                attach,
                attach_arc,
                viewer_id,
            );
            Some(WorkerReply::Empty)
        }
        Control::DetachTab => {
            if let Some((s, t, had_fwd)) = attach.detach() {
                if had_fwd {
                    registry.note_tab_output_observed(viewer_id, &s, &t);
                }
            }
            // A detached viewer has no focused tab, so their
            // viewport claim is stale — clear it so the PTY
            // re-aggregates to the remaining viewers' min (or lifts
            // the clamp entirely if this was the last viewer).
            registry.viewer_disconnected(viewer_id);
            Some(WorkerReply::Empty)
        }
        Control::Resize { cols, rows } | Control::TabResize { cols, rows } => {
            if let Some((section_id, tab_id)) = attach.snapshot_target() {
                registry.tab_resize(viewer_id, &section_id, &tab_id, cols, rows);
            }
            Some(WorkerReply::Empty)
        }
        Control::LaunchTab { section_id, tab_id } => {
            registry.launch_tab(&section_id, &tab_id);
            Some(WorkerReply::Empty)
        }

        // ── Read verbs ───────────────────────────────────────────────
        Control::ListProjects => Some(WorkerReply::ProjectList {
            projects: registry.list_projects(),
            ui: registry.ui_snapshot(),
        }),
        Control::ListProjectActions { project_id } => Some(WorkerReply::ProjectActionsAck {
            actions: registry.list_project_actions(&project_id),
        }),

        // ── Project mutation ─────────────────────────────────────────
        Control::AddProject { path } => Some(match registry.add_project(path).await {
            Ok(project) => WorkerReply::ProjectAdded { project },
            Err(e) => WorkerReply::Err {
                message: format!("{e:#}"),
                kind: ErrKind::Internal,
            },
        }),
        Control::RemoveProject { project_id } => Some(match registry.remove_project(&project_id) {
            Ok(()) => WorkerReply::ProjectRemoved { project_id },
            Err(e) => WorkerReply::Err {
                message: format!("{e:#}"),
                kind: ErrKind::Internal,
            },
        }),

        // ── Task & section mutation ──────────────────────────────────
        Control::SubmitNewTask {
            project_id,
            task_name,
            source_branch,
            agent_ids,
            branch_mode_existing,
            worktree_mode,
        } => Some(
            match registry
                .submit_new_task(
                    project_id,
                    task_name,
                    source_branch,
                    agent_ids,
                    branch_mode_existing,
                    worktree_mode,
                )
                .await
            {
                Ok(section_id) => WorkerReply::SubmitNewTaskAck { section_id },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::AddAgentToSection {
            section_id,
            agent_id,
        } => Some(
            match registry.add_agent_to_section(&section_id, &agent_id) {
                Ok(tab_id) => WorkerReply::AddAgentToSectionAck { tab_id },
                Err(message) => WorkerReply::Err {
                    kind: classify_unknown_id(&message),
                    message,
                },
            },
        ),
        Control::ActivateSectionTab { section_id, tab_id } => {
            Some(match registry.activate_section_tab(&section_id, &tab_id) {
                Ok(()) => WorkerReply::ActivateSectionTabAck,
                Err(message) => WorkerReply::Err {
                    kind: classify_unknown_id(&message),
                    message,
                },
            })
        }
        Control::CloseSectionTab { section_id, tab_id } => {
            Some(match registry.close_section_tab(&section_id, &tab_id) {
                Ok(active_tab_id) => WorkerReply::CloseSectionTabAck { active_tab_id },
                Err(message) => WorkerReply::Err {
                    kind: classify_unknown_id(&message),
                    message,
                },
            })
        }
        Control::ToggleSectionTabPinned { section_id, tab_id } => Some(
            match registry.toggle_section_tab_pinned(&section_id, &tab_id) {
                Ok(pinned) => WorkerReply::ToggleSectionTabPinnedAck { pinned },
                Err(message) => WorkerReply::Err {
                    kind: classify_unknown_id(&message),
                    message,
                },
            },
        ),
        Control::CreateWorktreeTask {
            project_id,
            task_name,
            source_branch,
            agent_provider,
        } => {
            let project_id_for_reply = project_id.clone();
            Some(
                match registry
                    .create_worktree_task(project_id, task_name, source_branch, agent_provider)
                    .await
                {
                    Ok(task) => WorkerReply::TaskCreated {
                        project_id: project_id_for_reply,
                        task,
                    },
                    Err(e) => WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    },
                },
            )
        }
        Control::RenameTask { task_id, new_name } => {
            let (changed, task) = registry.rename_task(&task_id, &new_name);
            Some(WorkerReply::TaskRenamed { changed, task })
        }
        Control::SetTaskPinned { task_id, pinned } => {
            let (changed, task) = registry.set_task_pinned(&task_id, pinned);
            Some(WorkerReply::TaskPinned { changed, task })
        }
        Control::RemoveTask {
            project_id,
            task_id,
        } => {
            let removed = registry.remove_task(&project_id, &task_id);
            Some(WorkerReply::TaskRemoved {
                project_id,
                task_id,
                removed,
            })
        }

        // ── Project actions ──────────────────────────────────────────
        Control::RunProjectAction {
            project_id,
            section_id,
            action_id,
        } => Some(
            match registry.run_project_action(&project_id, &section_id, &action_id) {
                Ok(tab_id) => WorkerReply::RunProjectActionAck { tab_id },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::SaveProjectAction {
            project_id,
            action,
            save_global_copy,
        } => Some(
            match registry.save_project_action(&project_id, action, save_global_copy) {
                Ok(()) => WorkerReply::SaveProjectActionAck,
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::DeleteProjectAction {
            project_id,
            action_id,
        } => {
            let deleted = registry.delete_project_action(&project_id, &action_id);
            Some(WorkerReply::DeleteProjectActionAck { deleted })
        }

        // ── Git read verbs ───────────────────────────────────────────
        Control::SlugifyBranchName { name } => Some(WorkerReply::SlugifyBranchNameAck {
            slug: registry.slugify_branch_name(&name),
        }),
        Control::ReadProjectBranches { project_id } => Some(WorkerReply::ProjectBranchesAck {
            branches: registry.read_project_branches(&project_id),
        }),
        Control::PrimaryBranchForProject { project_id } => Some(WorkerReply::PrimaryBranchAck {
            branch: registry.primary_branch_for_project(&project_id),
        }),
        Control::RepoDefaultCommitAction { project_id } => {
            Some(WorkerReply::RepoDefaultCommitActionAck {
                action: registry.repo_default_commit_action(&project_id),
            })
        }
        Control::ReadActiveGitState { project_id } => Some(WorkerReply::ActiveGitStateAck {
            state: registry.read_active_git_state(&project_id),
        }),
        Control::ReadChangedFiles { project_id } => Some(WorkerReply::ChangedFilesAck {
            files: registry.read_changed_files(&project_id),
        }),
        Control::ReadProjectGithubUrl { project_id } => Some(WorkerReply::ProjectGithubUrlAck {
            url: registry.read_project_github_url(&project_id),
        }),
        Control::ReadRecentCommits { project_id, limit } => Some(
            match registry.read_recent_commits(&project_id, limit as usize) {
                Ok(view) => WorkerReply::RecentCommitsAck { view },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::ReadCommitFileChanges {
            project_id,
            commit_id,
        } => Some(
            match registry.read_commit_file_changes(&project_id, &commit_id) {
                Ok(files) => WorkerReply::CommitFileChangesAck { files },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::ReadBranchSettings { project_id } => Some(WorkerReply::BranchSettingsAck {
            settings: registry.read_branch_settings(&project_id),
        }),
        Control::SetBranchSetting {
            project_id,
            field,
            branch_name,
        } => Some(
            match registry.set_branch_setting(&project_id, &field, branch_name.as_deref()) {
                Ok(changed) => WorkerReply::SetBranchSettingAck { changed },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            },
        ),

        // ── Git mutation verbs (changed-files / branches / actions) ──
        Control::StageChangedFile {
            project_id,
            path,
            original_path,
        } => Some(
            match registry
                .stage_changed_file(&project_id, &path, original_path.as_deref())
                .await
            {
                Ok(changed_files) => WorkerReply::StageChangedFileAck { changed_files },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::UnstageChangedFile {
            project_id,
            path,
            original_path,
        } => Some(
            match registry
                .unstage_changed_file(&project_id, &path, original_path.as_deref())
                .await
            {
                Ok(changed_files) => WorkerReply::UnstageChangedFileAck { changed_files },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::StageAllChanges { project_id } => {
            Some(match registry.stage_all_changes(&project_id).await {
                Ok(changed_files) => WorkerReply::StageAllChangesAck { changed_files },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            })
        }
        Control::UnstageAllChanges { project_id } => {
            Some(match registry.unstage_all_changes(&project_id).await {
                Ok(changed_files) => WorkerReply::UnstageAllChangesAck { changed_files },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            })
        }
        Control::DiscardChangedFile {
            project_id,
            path,
            untracked,
            original_path,
        } => Some(
            match registry
                .discard_changed_file(&project_id, &path, untracked, original_path.as_deref())
                .await
            {
                Ok(changed_files) => WorkerReply::DiscardChangedFileAck { changed_files },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::DiscardAllChanges { project_id, files } => Some(
            match registry.discard_all_changes(&project_id, files).await {
                Ok((changed_files, failures)) => WorkerReply::DiscardAllChangesAck {
                    changed_files,
                    failures,
                },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::RunToolbarGitAction {
            project_id,
            action_id,
        } => Some(
            match registry
                .run_toolbar_git_action(&project_id, &action_id)
                .await
            {
                Ok(outcome) => WorkerReply::ToolbarActionOutcomeAck { outcome },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::CreateBranch {
            project_id,
            branch_name,
            use_current_task,
            migrate_changes,
        } => Some(
            match registry
                .create_branch(&project_id, &branch_name, use_current_task, migrate_changes)
                .await
            {
                Ok((section_id, projects)) => WorkerReply::CreateBranchAck {
                    section_id,
                    projects,
                },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::CreateReviewTask {
            project_id,
            pull_request_number,
            head_branch,
            agent_provider,
        } => Some(
            match registry
                .create_review_task(
                    &project_id,
                    pull_request_number,
                    &head_branch,
                    agent_provider,
                )
                .await
            {
                Ok((section_id, projects)) => WorkerReply::CreateReviewTaskAck {
                    section_id,
                    projects,
                },
                Err(e) => WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                },
            },
        ),
        Control::FindPullRequestStatus { project_id } => {
            Some(match registry.find_pull_request_status(&project_id) {
                Ok(status) => WorkerReply::PullRequestStatusAck { status },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            })
        }
        Control::ReadPullRequestChecks { project_id } => {
            Some(match registry.read_pull_request_checks(&project_id) {
                Ok(checks) => WorkerReply::PullRequestChecksAck { checks },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            })
        }
        Control::FindProjectPullRequests {
            project_id,
            filter_index,
            query,
        } => Some(
            match registry.find_project_pull_requests(&project_id, filter_index, &query) {
                Ok(prs) => WorkerReply::ProjectPullRequestsAck { prs },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            },
        ),

        // ── Settings → Git Actions ───────────────────────────────────
        Control::ReadGitActionScripts => Some(WorkerReply::GitActionScriptsAck {
            view: registry.read_git_action_scripts(),
        }),
        Control::SetGitCommitScript { script } => {
            Some(match registry.set_git_commit_script(&script) {
                Ok(changed) => WorkerReply::SetGitCommitScriptAck { changed },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            })
        }
        Control::ResetGitCommitScript => Some(match registry.reset_git_commit_script() {
            Ok(changed) => WorkerReply::ResetGitCommitScriptAck { changed },
            Err(message) => WorkerReply::Err {
                message,
                kind: ErrKind::Internal,
            },
        }),
        Control::SetGitPrScript { script } => Some(match registry.set_git_pr_script(&script) {
            Ok(changed) => WorkerReply::SetGitPrScriptAck { changed },
            Err(message) => WorkerReply::Err {
                message,
                kind: ErrKind::Internal,
            },
        }),
        Control::ResetGitPrScript => Some(match registry.reset_git_pr_script() {
            Ok(changed) => WorkerReply::ResetGitPrScriptAck { changed },
            Err(message) => WorkerReply::Err {
                message,
                kind: ErrKind::Internal,
            },
        }),

        // ── Settings → Keybindings ───────────────────────────────────
        Control::ReadShortcutSettings => Some(WorkerReply::ShortcutSettingsAck {
            view: registry.read_shortcut_settings(),
        }),
        Control::SetShortcutBinding { action_id, binding } => {
            Some(match registry.set_shortcut_binding(&action_id, &binding) {
                Ok(()) => WorkerReply::SetShortcutBindingAck,
                Err(message) => WorkerReply::Err {
                    kind: classify_shortcut_action(&message),
                    message,
                },
            })
        }
        Control::ResetShortcutBinding { action_id } => {
            Some(match registry.reset_shortcut_binding(&action_id) {
                Ok(()) => WorkerReply::ResetShortcutBindingAck,
                Err(message) => WorkerReply::Err {
                    kind: classify_shortcut_action(&message),
                    message,
                },
            })
        }

        // ── Settings → MCP ───────────────────────────────────────────
        Control::ReadMcpSettings => Some(WorkerReply::McpSettingsAck {
            view: registry.read_mcp_settings(),
        }),
        Control::McpAddFromCatalog { catalog_id } => {
            Some(match registry.mcp_add_from_catalog(&catalog_id) {
                Ok(()) => WorkerReply::McpAddFromCatalogAck,
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            })
        }
        Control::McpToggle {
            entry_id,
            provider_id,
            enabled,
        } => Some(
            match registry.mcp_toggle(&entry_id, &provider_id, enabled) {
                Ok(()) => WorkerReply::McpToggleAck,
                Err(message) => {
                    let kind = if message.contains("unknown provider id") {
                        ErrKind::UnknownId
                    } else {
                        ErrKind::Internal
                    };
                    WorkerReply::Err { message, kind }
                }
            },
        ),
        Control::McpRemove { entry_id } => Some(match registry.mcp_remove(&entry_id) {
            Ok(()) => WorkerReply::McpRemoveAck,
            Err(message) => WorkerReply::Err {
                message,
                kind: ErrKind::Internal,
            },
        }),

        // ── Legacy / no-reply / pre-handled ──────────────────────────
        Control::WatchProject { project_path: _ } => {
            // Legacy no-op. Kept in the enum for serde-compat with
            // any lingering clients; new clients use ListProjects +
            // AttachTab.
            debug!("legacy Control::WatchProject ignored");
            None
        }
        Control::Hello { .. } => {
            // Hello is the dial-time pairing handshake — concrete
            // transports consume it before constructing the
            // ServerSession the dispatcher sees. A stray Hello mid-
            // session is harmless; ignore quietly to match the legacy
            // behaviour. We don't surface an error reply because
            // pre-cutover the iroh path didn't either.
            debug!("stray Control::Hello reached dispatcher; ignored");
            None
        }
        Control::ReadEnabledAgents
        | Control::ReadAgentSettings
        | Control::SetAgentEnabled { .. }
        | Control::SetDefaultAgent { .. }
        | Control::SetAgentLaunchArgs { .. }
        | Control::OpenInState
        | Control::ReadOpenInSettings
        | Control::SetOpenInAppEnabled { .. }
        | Control::OpenProjectInApp { .. } => {
            unreachable!("domain command should be handled by per-domain helper above")
        }
    }
}

/// Wire up an `AttachTab` request: drop any prior attachment, ask the
/// registry for a fresh broadcast subscription, and spawn a forwarder
/// task that pumps bytes into `session.push_data`.
///
/// Two paths through the registry: `attach_tab_with_replay` returns an
/// `Option<TabAttachment>`. `Some` means a live PTY is running and we
/// got a receiver + replay buffer; `None` means the tab is launching
/// (or doesn't exist) — we record the attach intent but defer
/// installing a forwarder until the client retries.
#[allow(clippy::too_many_arguments)]
fn handle_attach(
    section_id: String,
    tab_id: String,
    registry: &dyn DaemonRegistry,
    _session: &dyn ServerSession,
    session_arc: Arc<dyn ServerSession>,
    attach: &AttachState,
    attach_arc: Arc<AttachState>,
    viewer_id: &str,
) {
    // Are we re-attaching to the same target? If so, we *don't* bump
    // the generation — the existing forwarder would lose its view of
    // its own generation and silently stop pushing. Instead we keep
    // the existing generation so the new forwarder we spawn here
    // matches.
    let same_target = {
        let inner = attach.inner.lock().expect("attach state poisoned");
        inner.section_id.as_deref() == Some(section_id.as_str())
            && inner.tab_id.as_deref() == Some(tab_id.as_str())
    };

    let generation = if same_target {
        attach.generation.load(Ordering::Relaxed)
    } else {
        attach
            .generation
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
    };

    // Drop any prior attachment on this connection.
    let prior = {
        let mut inner = attach.inner.lock().expect("attach state poisoned");
        let prior_target = match (inner.section_id.take(), inner.tab_id.take()) {
            (Some(s), Some(t)) => Some((s, t, inner.forwarder.is_some())),
            _ => None,
        };
        if let Some(handle) = inner.forwarder.take() {
            handle.abort();
        }
        prior_target
    };
    if let Some((s, t, had_fwd)) = prior {
        if had_fwd {
            registry.note_tab_output_observed(viewer_id, &s, &t);
        }
    }

    // Switching to a different tab: clear this viewer's viewport claim
    // before installing a new one. Without this, switching attach
    // targets leaves the old tab's `active_viewers` entry stale until
    // the first TabResize arrives.
    if !same_target {
        registry.viewer_disconnected(viewer_id);
    }

    let Some(attachment) = registry.attach_tab_with_replay(viewer_id, &section_id, &tab_id) else {
        // Live runtime not ready; record the attach but skip the
        // forwarder. Client will see a stalled stream until it
        // retries.
        debug!(section_id, tab_id, "attach_tab: waiting for live runtime");
        let mut inner = attach.inner.lock().expect("attach state poisoned");
        inner.section_id = Some(section_id);
        inner.tab_id = Some(tab_id);
        inner.forwarder = None;
        return;
    };

    let mut rx = attachment.receiver;
    let replay = attachment.replay;
    let section_id_for_task = section_id.clone();
    let tab_id_for_task = tab_id.clone();
    let attach_for_task = attach_arc;
    let session_for_task = session_arc;
    let forwarder = tokio::spawn(async move {
        for bytes in replay {
            if attach_for_task.current_generation() != generation {
                return;
            }
            if session_for_task
                .push_data(&section_id_for_task, &tab_id_for_task, &bytes)
                .await
                .is_err()
            {
                return;
            }
        }
        loop {
            match rx.recv().await {
                Ok(bytes) => {
                    if attach_for_task.current_generation() != generation {
                        return;
                    }
                    if session_for_task
                        .push_data(&section_id_for_task, &tab_id_for_task, &bytes)
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    // Slow consumer dropped `n` chunks. There's no
                    // in-band resync we can perform; tear down the
                    // attachment and force a reattach so the client
                    // gets a fresh scrollback replay + clean VT
                    // state.
                    warn!(
                        lagged = n,
                        "attach forwarder lagged; dropping attachment to force reattach"
                    );
                    break;
                }
            }
        }
    });

    let mut inner = attach.inner.lock().expect("attach state poisoned");
    inner.section_id = Some(section_id);
    inner.tab_id = Some(tab_id);
    inner.forwarder = Some(forwarder.abort_handle());
}

/// Common heuristic for verbs whose error string distinguishes
/// unknown-id failures from internal failures by substring match.
/// Mirrors the legacy `transport_iroh::handle_control` classification.
fn classify_unknown_id(message: &str) -> ErrKind {
    if message.contains("unknown") || message.contains("malformed") {
        ErrKind::UnknownId
    } else {
        ErrKind::Internal
    }
}

fn classify_shortcut_action(message: &str) -> ErrKind {
    if message.contains("unknown action id") {
        ErrKind::UnknownId
    } else {
        ErrKind::Internal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daemon_proto::ProjectSummary;
    use daemon_transport::in_memory::pair;
    #[allow(unused_imports)]
    use daemon_transport::Session as _;
    use std::sync::Mutex;

    /// Minimal registry stub — health is OK; list_projects returns a
    /// fixed slice. Everything else uses default impls so the test
    /// fails loudly via WorkerReply::Err if dispatch routes to an
    /// unimplemented verb the test doesn't expect.
    struct StubRegistry {
        projects: Mutex<Vec<ProjectSummary>>,
        slugified: Mutex<Option<String>>,
    }

    impl StubRegistry {
        fn new() -> Self {
            Self {
                projects: Mutex::new(vec![ProjectSummary {
                    id: "p1".into(),
                    name: "p1".into(),
                    path: "/tmp/p1".into(),
                    kind: daemon_proto::ProjectKind::Root,
                    current_branch: Some("main".into()),
                    tasks: vec![],
                }]),
                slugified: Mutex::new(None),
            }
        }
    }

    impl DaemonRegistry for StubRegistry {
        fn health(&self) -> Result<(), String> {
            Ok(())
        }
        fn list_projects(&self) -> Vec<ProjectSummary> {
            self.projects.lock().unwrap().clone()
        }
        fn attach_tab(
            &self,
            _: &str,
            _: &str,
        ) -> Option<tokio::sync::broadcast::Receiver<Vec<u8>>> {
            None
        }
        fn tab_input(&self, _: &str, _: &str, _: &[u8]) {}
        fn tab_resize(&self, _: &str, _: &str, _: &str, _: u16, _: u16) {}
        fn slugify_branch_name(&self, name: &str) -> String {
            let slug = format!("slug-of-{name}");
            *self.slugified.lock().unwrap() = Some(slug.clone());
            slug
        }
        fn read_project_branches(&self, _project_id: &str) -> Vec<String> {
            vec!["main".into(), "feat/x".into()]
        }
    }

    fn server_arc(server: Box<dyn ServerSession>) -> Arc<dyn ServerSession> {
        // Box<dyn _> -> Arc<dyn _>: the easiest path through the type
        // system is to convert via `Arc::from(Box::leak(...))` — but
        // we want proper drop, so use Arc::from on the boxed dyn.
        // `Arc::from` is implemented for `Box<T>` for `T: ?Sized`.
        Arc::from(server)
    }

    #[tokio::test]
    async fn serve_session_round_trips_list_projects() {
        let (server, client) = pair("test-peer");
        let registry: Arc<dyn DaemonRegistry> = Arc::new(StubRegistry::new());
        let server_task = tokio::spawn(serve_session(server_arc(server), Arc::clone(&registry)));

        let reply = client.call(Control::ListProjects).await.expect("call");
        match reply {
            WorkerReply::ProjectList { projects } => {
                assert_eq!(projects.len(), 1);
                assert_eq!(projects[0].id, "p1");
            }
            other => panic!("expected ProjectList, got {other:?}"),
        }

        drop(client);
        server_task
            .await
            .expect("serve task")
            .expect("serve_session result");
    }

    #[tokio::test]
    async fn serve_session_dispatches_slugify_branch_name() {
        let (server, client) = pair("test-peer");
        let registry: Arc<dyn DaemonRegistry> = Arc::new(StubRegistry::new());
        let _server_task = tokio::spawn(serve_session(server_arc(server), Arc::clone(&registry)));

        let reply = client
            .call(Control::SlugifyBranchName {
                name: "Add cool feature".into(),
            })
            .await
            .expect("call");
        match reply {
            WorkerReply::SlugifyBranchNameAck { slug } => {
                assert_eq!(slug, "slug-of-Add cool feature");
            }
            other => panic!("expected SlugifyBranchNameAck, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn serve_session_dispatches_read_project_branches() {
        let (server, client) = pair("test-peer");
        let registry: Arc<dyn DaemonRegistry> = Arc::new(StubRegistry::new());
        let _server_task = tokio::spawn(serve_session(server_arc(server), Arc::clone(&registry)));

        let reply = client
            .call(Control::ReadProjectBranches {
                project_id: "p1".into(),
            })
            .await
            .expect("call");
        match reply {
            WorkerReply::ProjectBranchesAck { branches } => {
                assert_eq!(branches, vec!["main".to_string(), "feat/x".to_string()]);
            }
            other => panic!("expected ProjectBranchesAck, got {other:?}"),
        }
    }

    /// Health-failed registry routes everything to WorkerReply::Err.
    struct UnhealthyRegistry;
    impl DaemonRegistry for UnhealthyRegistry {
        fn health(&self) -> Result<(), String> {
            Err("registry unavailable".into())
        }
        fn list_projects(&self) -> Vec<ProjectSummary> {
            panic!("should not be called when health fails");
        }
        fn attach_tab(
            &self,
            _: &str,
            _: &str,
        ) -> Option<tokio::sync::broadcast::Receiver<Vec<u8>>> {
            None
        }
        fn tab_input(&self, _: &str, _: &str, _: &[u8]) {}
        fn tab_resize(&self, _: &str, _: &str, _: &str, _: u16, _: u16) {}
    }

    #[tokio::test]
    async fn serve_session_returns_internal_err_when_registry_unhealthy() {
        let (server, client) = pair("test-peer");
        let registry: Arc<dyn DaemonRegistry> = Arc::new(UnhealthyRegistry);
        let _server_task = tokio::spawn(serve_session(server_arc(server), registry));

        let reply = client.call(Control::ListProjects).await.expect("call");
        match reply {
            WorkerReply::Err { message, kind } => {
                assert_eq!(message, "registry unavailable");
                assert!(matches!(kind, ErrKind::Internal));
            }
            other => panic!("expected Err, got {other:?}"),
        }
    }
}
